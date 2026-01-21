# TODO / Fix Plan: `EXCHANGE_CREATE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_CREATE_CONTRACT.planning.md`.

## 0) Decide “parity target” (do this first)

- [ ] Confirm scope:
  - [ ] **Actuator-only parity** (match `ExchangeCreateActuator` + TRC-10 balance helpers + receipts)
  - [ ] **End-to-end parity** (also cover any surrounding processors if remote execution is expected to fully mirror embedded behavior)
- [ ] Confirm supported network/property matrix:
  - [ ] `ALLOW_SAME_TOKEN_NAME == 1` only (mainnet-modern)
  - [ ] must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay)
  - [ ] must support `ALLOW_ASSET_OPTIMIZATION == 1` (balances in `AccountAssetStore`)
  - [ ] must support `ALLOW_BLACKHOLE_OPTIMIZATION == 1` (burn counter)

## 1) Receipt parity (required)

Goal: match Java’s `ret.setStatus(fee, SUCESS)` + `ret.setExchangeId(id)` serialization.

- [ ] In `execute_exchange_create_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] Build receipt with **both** fields:
    - [ ] `.with_fee(exchange_create_fee)`
    - [ ] `.with_exchange_id(exchange_id)`
  - [ ] (Optional) explicitly set `.with_ret(0)` if you want the field present; note proto3 typically omits default `0` anyway.
- [ ] Add a small Rust test that decodes the returned `tron_transaction_result` bytes and asserts:
  - [ ] field 1 == exchange_create_fee
  - [ ] field 21 == exchange_id
  - [ ] matches fixture `conformance/fixtures/exchange_create_contract/*/expected/result.pb`

## 2) Fee sink parity: burn vs blackhole credit (required)

Goal: match `DynamicPropertiesStore.supportBlackHoleOptimization()` behavior:

- if `ALLOW_BLACKHOLE_OPTIMIZATION == 1`: increment `BURN_TRX_AMOUNT`
- else: credit blackhole account

Checklist:

- [ ] In `execute_exchange_create_contract()`:
  - [ ] Replace the current fee sink block with the standard pattern already used in other handlers (e.g. MarketCancelOrder):
    - [ ] if `support_blackhole == true`: `storage_adapter.burn_trx(exchange_create_fee as u64)`
    - [ ] else: `storage_adapter.add_balance(blackhole, exchange_create_fee as u64)`
  - [ ] Ensure errors propagate (don’t `unwrap_or(true)` on a DB read failure unless explicitly desired)
- [ ] Add tests:
  - [ ] blackhole optimization disabled → blackhole account balance increases, `BURN_TRX_AMOUNT` unchanged
  - [ ] blackhole optimization enabled → `BURN_TRX_AMOUNT` increases, blackhole account unchanged

## 3) TRC-10 balance validation parity (required if enabling outside the “easy” config)

Goal: replicate Java’s `AccountCapsule.assetBalanceEnoughV2()` semantics:

- allowSameTokenName==0: read `Account.asset[name]`
- allowSameTokenName==1: read `Account.asset_v2[token_id]`
- when `ALLOW_ASSET_OPTIMIZATION == 1`, balances may live in `AccountAssetStore` and must be imported/queried

Checklist:

- [ ] Decide approach:
  - [ ] **Minimal** (only support allowSameTokenName==1, asset optimization disabled)
  - [ ] **Full parity** (support allowSameTokenName==0 and asset optimization)
- [ ] If full parity:
  - [ ] Add a Rust helper that mirrors `assetBalanceEnoughV2()`:
    - [ ] accept `(account_proto, key_bytes, allow_same_token_name, allow_asset_optimization, account_asset_store)` (or hide behind storage adapter)
  - [ ] Implement/extend storage adapter support for the `AccountAssetStore` DB if the backend is expected to run with `ALLOW_ASSET_OPTIMIZATION == 1`.
  - [ ] Update ExchangeCreate validation to use this helper instead of `get_asset_balance_v2()`.
- [ ] Add tests for validation behavior:
  - [ ] allowSameTokenName==0 token name key present in `Account.asset` → validate passes
  - [ ] allowSameTokenName==1 token id present in `Account.asset_v2` → validate passes
  - [ ] allowAssetOptimization==1 and balance present only in account-asset store → validate passes

## 4) Fix `EXCHANGE_CREATE_FEE` fallback default (required)

Goal: align Rust fallback with Java’s initialization value.

- [ ] In `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
  - [ ] Change the fallback default for `get_exchange_create_fee()` to `1024000000` (1024 TRX in SUN)
  - [ ] Fix the comment to match Java
- [ ] Add a unit test for “missing key” behavior (if you keep defaults):
  - [ ] missing `EXCHANGE_CREATE_FEE` → returns `1024000000`

## 5) Verification steps (before enabling in config)

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run the conformance runner for `exchange_create_contract` fixtures with `exchange_create_enabled=true`
- [ ] Java (optional, if running in dual/remote mode):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`
  - [ ] Validate receipt bytes parsed by `ExecutionProgramResult.fromExecutionResult()` include fee + exchange_id

## 6) Rollout checklist (after fixes)

- [ ] Keep `exchange_create_enabled` default `false` until conformance passes
- [ ] Enable it in controlled environments (devnet / conformance runs) first
- [ ] Only then consider turning it on in production configs

