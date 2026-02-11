# TODO / Fix Plan: `EXCHANGE_CREATE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_CREATE_CONTRACT.planning.md`.

## 0) Decide "parity target" (do this first)

- [x] Confirm scope:
  - [x] **Actuator-only parity** (match `ExchangeCreateActuator` + TRC-10 balance helpers + receipts)
  - [x] **End-to-end parity** (also cover any surrounding processors if remote execution is expected to fully mirror embedded behavior)
- [x] Confirm supported network/property matrix:
  - [x] `ALLOW_SAME_TOKEN_NAME == 1` only (mainnet-modern)
  - [x] must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay)
  - [x] must support `ALLOW_ASSET_OPTIMIZATION == 1` (balances in `AccountAssetStore`)
  - [x] must support `ALLOW_BLACKHOLE_OPTIMIZATION == 1` (burn counter)

## 1) Receipt parity (required)

Goal: match Java's `ret.setStatus(fee, SUCESS)` + `ret.setExchangeId(id)` serialization.

- [x] In `execute_exchange_create_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] Build receipt with **both** fields:
    - [x] `.with_fee(exchange_create_fee)`
    - [x] `.with_exchange_id(exchange_id)`
  - [ ] (Optional) explicitly set `.with_ret(0)` if you want the field present; note proto3 typically omits default `0` anyway.
- [x] Add a small Rust test that decodes the returned `tron_transaction_result` bytes and asserts:
  - [x] field 1 == exchange_create_fee
  - [x] field 21 == exchange_id
  - [x] matches fixture `conformance/fixtures/exchange_create_contract/*/expected/result.pb` (`test_exchange_create_receipt_matches_conformance_fixture`)

## 2) Fee sink parity: burn vs blackhole credit (required)

Goal: match `DynamicPropertiesStore.supportBlackHoleOptimization()` behavior:

- if `ALLOW_BLACKHOLE_OPTIMIZATION == 1`: increment `BURN_TRX_AMOUNT`
- else: credit blackhole account

Checklist:

- [x] In `execute_exchange_create_contract()`:
  - [x] Replace the current fee sink block with the standard pattern already used in other handlers (e.g. MarketCancelOrder):
    - [x] if `support_blackhole == true`: `storage_adapter.burn_trx(exchange_create_fee as u64)`
    - [x] else: `storage_adapter.add_balance(blackhole, exchange_create_fee as u64)`
  - [x] Ensure errors propagate (don't `unwrap_or(true)` on a DB read failure unless explicitly desired)
- [x] Add tests:
  - [x] blackhole optimization disabled → blackhole account balance increases, `BURN_TRX_AMOUNT` unchanged
  - [x] blackhole optimization enabled → `BURN_TRX_AMOUNT` increases, blackhole account unchanged

## 3) TRC-10 balance validation parity (required if enabling outside the "easy" config)

Goal: replicate Java's `AccountCapsule.assetBalanceEnoughV2()` semantics:

- allowSameTokenName==0: read `Account.asset[name]`
- allowSameTokenName==1: read `Account.asset_v2[token_id]`
- when `ALLOW_ASSET_OPTIMIZATION == 1`, balances may live in `AccountAssetStore` and must be imported/queried

Checklist:

- [x] Decide approach:
  - [ ] **Minimal** (only support allowSameTokenName==1, asset optimization disabled)
  - [x] **Full parity** (support allowSameTokenName==0 and asset optimization)
- [x] If full parity:
  - [x] Add a Rust helper that mirrors `assetBalanceEnoughV2()`:
    - [x] accept `(account_proto, key_bytes, allow_same_token_name, allow_asset_optimization, account_asset_store)` (or hide behind storage adapter)
    - [x] Implemented `asset_balance_enough_v2()` in `engine.rs` that handles all modes
  - [x] Implement/extend storage adapter support for the `AccountAssetStore` DB if the backend is expected to run with `ALLOW_ASSET_OPTIMIZATION == 1`.
    - [x] Added `ACCOUNT_ASSET` constant in `db_names.rs`
    - [x] Added `get_allow_asset_optimization()` method
    - [x] Added `get_asset_balance_from_asset_store()` method
    - [x] Added `import_asset_if_optimized()` helper in `mod.rs` (mirrors Java's `importAsset()`)
  - [x] Update ExchangeCreate validation to use this helper instead of `get_asset_balance_v2()`.
- [x] Add tests for validation behavior:
  - [x] allowSameTokenName==0 token name key present in `Account.asset` → validate passes (`test_exchange_create_legacy_mode_reads_asset_map`)
  - [x] allowSameTokenName==1 token id present in `Account.asset_v2` → validate passes (`test_exchange_create_receipt_includes_fee_and_exchange_id`)
  - [x] allowAssetOptimization==1 and balance present only in account-asset store → validate passes (`test_exchange_create_asset_optimization_reads_asset_store`)

## 4) Fix `EXCHANGE_CREATE_FEE` fallback default (required)

Goal: align Rust fallback with Java's initialization value.

- [x] In `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
  - [x] Change the fallback default for `get_exchange_create_fee()` to `1024000000` (1024 TRX in SUN)
  - [x] Fix the comment to match Java
- [x] Add a unit test for "missing key" behavior (if you keep defaults):
  - [x] missing `EXCHANGE_CREATE_FEE` → returns `1024000000` (`test_exchange_create_fee_default_when_missing`)

## 5) End-to-end parity tests (surrounding processors)

Goal: Verify state changes match Java's behavior after full transaction execution.

- [x] Add tests for state changes after execution:
  - [x] Owner TRX balance deduction: fee + TRX deposit (`test_exchange_create_deducts_owner_trx_balance`)
  - [x] Owner TRC-10 balance deduction (`test_exchange_create_deducts_owner_trc10_balance`)
  - [x] Exchange record storage in ExchangeV2Store (`test_exchange_create_stores_exchange_record`)
  - [x] Exchange record content verification (creator, balances, token IDs)
  - [x] Token-to-token exchange (no TRX involved) (`test_exchange_create_token_to_token`)
  - [x] LATEST_EXCHANGE_NUM update (`test_exchange_create_increments_exchange_id`)
- [x] Add validation error tests:
  - [x] Insufficient balance for fee (`test_exchange_create_fails_insufficient_balance_for_fee`)
  - [x] Insufficient TRX for deposit (`test_exchange_create_fails_insufficient_trx_for_deposit`)
  - [x] Insufficient TRC-10 balance (`test_exchange_create_fails_insufficient_trc10_balance`)
  - [x] Same tokens (`test_exchange_create_fails_same_tokens`)
  - [x] Zero balance (`test_exchange_create_fails_zero_balance`)
  - [x] Balance exceeds limit (`test_exchange_create_fails_balance_exceeds_limit`)
  - [x] Invalid token ID (`test_exchange_create_fails_invalid_token_id`)

## 6) Verification steps (before enabling in config)

- [x] Rust:
  - [x] `cd rust-backend && cargo test` (exchange_create tests pass - 19 tests)
  - [ ] Run the conformance runner for `exchange_create_contract` fixtures with `exchange_create_enabled=true`
- [ ] Java (optional, if running in dual/remote mode):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`
  - [ ] Validate receipt bytes parsed by `ExecutionProgramResult.fromExecutionResult()` include fee + exchange_id

## 7) Rollout checklist (after fixes)

- [ ] Keep `exchange_create_enabled` default `false` until conformance passes
- [ ] Enable it in controlled environments (devnet / conformance runs) first
- [ ] Only then consider turning it on in production configs

