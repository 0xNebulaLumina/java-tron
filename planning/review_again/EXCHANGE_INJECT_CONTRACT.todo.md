# TODO / Fix Plan: `EXCHANGE_INJECT_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_INJECT_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm which modes must be supported:
  - [ ] Only `ALLOW_SAME_TOKEN_NAME == 1` (modern/mainnet replay after the proposal)
  - [ ] Must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay)
  - [ ] Must support `ALLOW_ASSET_OPTIMIZATION == 1` / account-asset store import semantics
- [ ] Confirm what “parity” means operationally:
  - [ ] correctness of state (exchange + account DB contents)
  - [ ] exact error strings
  - [ ] receipt bytes/fields

## 1) Fix exchange store routing (required for `ALLOW_SAME_TOKEN_NAME == 0`)

Goal: mirror `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()`.

- [ ] In `execute_exchange_inject_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] When `allow_same_token_name == 0`:
    - [ ] load the exchange from v1: `storage_adapter.get_exchange_from_store(exchange_id, false)`
    - [ ] validate token membership against v1 token **names**
    - [ ] after updating balances, persist:
      - [ ] v1 exchange to `exchange` via `put_exchange_to_store(..., false)`
      - [ ] v2 copy to `exchange-v2`:
        - [ ] resolve token names → ids using `get_asset_issue(name, 0).id` (skip TRX `"_"`)
        - [ ] store via `put_exchange(...)`
  - [ ] When `allow_same_token_name == 1`:
    - [ ] keep the current v2-only behavior (read + write v2)

Notes:
- Java does not “backfill” v1 once allowSameTokenName==1; v1 is effectively frozen. Preserve that behavior.

## 2) Fix TRC-10 balance validation (required for legacy mode; also needed for asset optimization)

Goal: mirror `AccountCapsule.assetBalanceEnoughV2()` semantics.

- [ ] Replace `storage_adapter.get_asset_balance_v2(address, token_bytes)` calls in validation with a helper that routes by `allow_same_token_name`:
  - [ ] allow==0 → read `account.asset[name]`
  - [ ] allow==1 → read `account.asset_v2[token_id]`
  - [ ] (Optional but recommended) reuse `Self::get_asset_balance_v2(account_proto, key_bytes, allow)` already present in `mod.rs`
- [ ] If asset optimization must be supported:
  - [ ] implement account-asset store lookups/import equivalent to Java’s `importAsset(key)`
  - [ ] update the helper to consult the account-asset store when enabled

## 3) Align missing-owner error string (optional but improves parity)

- [ ] Change `"Owner account not found"` to Java-style:
  - [ ] `account[<hex-address>] not exists`
  - [ ] ensure the same address formatting used by other conformance fixtures (`StringUtil.createReadableString` → hex)

## 4) Add/extend conformance coverage (recommended)

Goal: ensure we don’t regress and that legacy mode is actually validated.

- [ ] Add conformance fixtures for `ALLOW_SAME_TOKEN_NAME == 0`:
  - [ ] inject using token **names** (non-TRX/ non-TRX case, e.g. `"abc"`)
  - [ ] inject on TRX side with token-name other side (`"_"` + `"def"`)
  - [ ] assert both `exchange` and `exchange-v2` post-state matches Java expectations
- [ ] Add at least one “true happy path” success fixture that does **not** overflow in execute:
  - [ ] validate success + post-state updates
  - [ ] receipt includes `exchange_inject_another_amount`

## 5) Verification steps (before enabling in config)

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] run the conformance runner for `exchange_inject_contract` fixtures with `exchange_inject_enabled=true`
- [ ] Java (optional, if validating remote mode end-to-end):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ExchangeInjectActuatorTest"`

## 6) Rollout checklist

- [ ] Keep `exchange_inject_enabled` default `false` until legacy-mode fixtures (if required) pass
- [ ] Enable in dev/conformance environments first, then consider production configs

