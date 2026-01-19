# TODO / Fix Plan: `EXCHANGE_TRANSACTION_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_TRANSACTION_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm which network/mode(s) Rust must support:
  - [ ] Only `ALLOW_SAME_TOKEN_NAME == 1` (modern/mainnet behavior; exchange-v2 only)
  - [ ] Must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay; exchange store + name keys)
  - [ ] Must support `ALLOW_ASSET_OPTIMIZATION == 1` / `importAsset(key)` semantics
- [ ] Confirm what “parity” means for this contract:
  - [ ] correct **state** (account + exchange DB contents)
  - [ ] exact **error strings** and **error ordering**
  - [ ] exact **receipt bytes** (`Transaction.Result` encoding)
  - [ ] deterministic math parity when `ALLOW_STRICT_MATH == 1`

## 1) Fix exchange store routing for `ALLOW_SAME_TOKEN_NAME == 0` (required for legacy parity)

Goal: mirror `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()`.

- [ ] In `execute_exchange_transaction_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] Read `allow_same_token_name` **before** loading the exchange
  - [ ] When `allow_same_token_name == 0`:
    - [ ] load exchange from v1 store: `storage_adapter.get_exchange_from_store(exchange_id, false)`
    - [ ] validate token membership against v1 token **names**
    - [ ] after updating balances, persist:
      - [ ] v1 exchange to `exchange` via `put_exchange_to_store(..., false)`
      - [ ] v2 exchange to `exchange-v2`:
        - [ ] clone the updated exchange
        - [ ] resolve token name → token id for each side using `get_asset_issue(name, 0).id` (skip TRX `"_"`)
        - [ ] write via `put_exchange_to_store(..., true)` (or `put_exchange(...)`)
  - [ ] When `allow_same_token_name == 1`:
    - [ ] keep the current v2-only read/write behavior

Notes:
- Java keeps v1 “frozen” once allowSameTokenName is enabled; avoid backfilling v1 in allow==1 mode.

## 2) Fix TRC-10 balance validation (legacy + correctness)

Goal: mirror `AccountCapsule.assetBalanceEnoughV2()` semantics.

- [ ] Replace `storage_adapter.get_asset_balance_v2(...)` usage in validation with an allow-aware helper:
  - [ ] allow==0 → read `account.asset[name]`
  - [ ] allow==1 → read `account.asset_v2[id]`
  - [ ] Prefer reusing `Self::get_asset_balance_v2(account_proto, key_bytes, allow)` already present in `mod.rs`
- [ ] If `ALLOW_ASSET_OPTIMIZATION == 1` must be supported:
  - [ ] implement the equivalent of Java’s `importAsset(key)` path (account-asset store hydration)
  - [ ] ensure the balance helper consults the account-asset store when required

## 3) Fix TRC-10 credit/debit keying in legacy mode (avoid writing `asset["<id>"]`)

Goal: when allow==0 and the contract uses token **names**, ensure the “name map” (`Account.asset`) is updated under the token name key, not the numeric id.

- [ ] Ensure legacy execution uses v1 exchange token ids (names) so `another_token_id` is also the name.
- [ ] Ensure the `token_id_str` / `another_token_id_str` resolution only affects the `asset_v2` key, not the `asset` name key.

## 4) StrictMath parity hardening (if `ALLOW_STRICT_MATH == 1` is consensus-relevant)

Goal: match Java `StrictMath.pow` bit-for-bit in strict mode.

- [ ] Replace the strict branch in `ExchangeProcessor::pow()` (`rust-backend/crates/core/src/service/contracts/exchange.rs`) with a deterministic implementation that matches fdlibm semantics
  - [ ] evaluate options:
    - [ ] use a vetted fdlibm port crate
    - [ ] vendor/port the relevant fdlibm pow implementation (with tests)
- [ ] Add targeted regression tests with vectors that are known to be rounding-sensitive after the `(double)->long` truncation
- [ ] Add/extend conformance fixtures where `ALLOW_STRICT_MATH == 1` and the expected received amount differs if pow rounding drifts

## 5) Owner-address validation parity (optional but improves correctness/error ordering)

Goal: match Java’s early `"Invalid address"` behavior and ensure contract owner matches tx sender.

- [ ] In `parse_exchange_transaction_contract()`:
  - [ ] decode and return `owner_address` (do not skip field 1)
- [ ] In `execute_exchange_transaction_contract()`:
  - [ ] validate owner_address length/prefix (21 bytes, correct network prefix)
  - [ ] validate `owner_address` corresponds to `transaction.from` (or derive owner from owner_address and use that consistently)
- [ ] Align missing-owner error message to Java style:
  - [ ] `account[<readable>] not exists` (and match address formatting conventions used elsewhere)

## 6) Receipt parity (only if byte-for-byte matching matters)

- [ ] Decide whether to always set `fee` and `ret` in `TransactionResultBuilder` for system contracts (even when fee==0)
- [ ] Consider extending the Rust conformance runner to compare `expected/result.pb` for contracts that emit receipts

## 7) Add conformance coverage for legacy mode (recommended if allow==0 must work)

- [ ] Add fixtures for `ALLOW_SAME_TOKEN_NAME == 0`:
  - [ ] TRX → token-name swap (`"_"` ↔ `"abc"`) asserting:
    - [ ] v1 exchange (`exchange`) updated with token names
    - [ ] v2 exchange (`exchange-v2`) updated with token ids
    - [ ] account `asset["abc"]` updated (not `asset["1"]`)
  - [ ] token-name → token-name swap (`"abc"` ↔ `"def"`)
  - [ ] legacy failure modes (wrong token, insufficient token balance, slippage) using token names

## 8) Verification steps

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] run conformance runner on:
    - [ ] `exchange_transaction_contract` fixtures (existing)
    - [ ] new legacy fixtures (if added)
- [ ] Java (optional end-to-end validation):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ExchangeTransactionActuatorTest"`

## 9) Rollout checklist

- [ ] If legacy parity is not implemented, gate execution:
  - [ ] when `ALLOW_SAME_TOKEN_NAME == 0`, force fallback to Java (and document the limitation)
- [ ] Keep `exchange_transaction_enabled` default `false` until the chosen parity target is fully met

