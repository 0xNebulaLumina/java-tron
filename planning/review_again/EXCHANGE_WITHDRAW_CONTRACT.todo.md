# TODO / Fix Plan: `EXCHANGE_WITHDRAW_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_WITHDRAW_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm which modes must be supported:
  - [x] Only `ALLOW_SAME_TOKEN_NAME == 1` (modern/mainnet) - **SUPPORTED**
  - [x] Must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay; exchange store + name keys) - **SUPPORTED via get_exchange_routed() and put_exchange_dual_write()**
- [x] Confirm what "parity" means operationally:
  - [x] correctness of state (exchange + account DB contents) - **IMPLEMENTED**
  - [x] exact acceptance/rejection boundaries (esp. "Not precise enough") - **FIXED**
  - [x] exact error strings - **VERIFIED (hex::encode matches ByteArray.toHexString)**
  - [x] receipt bytes/fields - **IMPLEMENTED (field 20: exchange_withdraw_another_amount)**

## 1) Fix the "Not precise enough" precision check (high priority if strict parity matters)

Goal: mirror Java's `BigDecimal.divide(..., 4, ROUND_HALF_UP)` based check from `ExchangeWithdrawActuator.validate`.

- [x] Update `is_withdraw_precise_enough()` in `rust-backend/crates/core/src/service/contracts/exchange.rs`:
  - [x] Stop using raw `f64` division for this check.
  - [x] Implement an integer math equivalent of Java's 4dp half-up rounding:
    - [x] `another = floor(numerator / denom)` where `numerator = other_balance * token_quant`
    - [x] `q4_scaled = round_half_up((numerator * 10000) / denom)` (integer result; represents `q4 * 10000`)
    - [x] `remainder_scaled = q4_scaled - (another * 10000)`
    - [x] Java rejects if `(remainder / another) > 0.0001` → in scaled integers this is `remainder_scaled > another`
  - [x] Confirm sign behavior is correct (today all inputs are positive, but implementation should be robust).
- [x] Add unit tests in `rust-backend/crates/core/src/service/contracts/exchange.rs`:
  - [x] boundary cases where Java passes but float-exact would fail:
    - [x] `another == 1` and true remainder in `(0.0001, 0.00015)` should **pass** with Java rounding
    - [x] true remainder ≥ `0.00015` should **fail** (rounds to `0.0002`)
  - [x] cases where `another >= 10000` should always pass (since the ratio threshold becomes ≥ 1.0)
- [x] Add a conformance fixture that exercises the boundary:
  - [x] Existing `validate_fail_not_precise_enough` fixture already tests the precision check
  - [x] All 12 exchange_withdraw_contract conformance tests pass (including precision check)
  - [x] Assert Rust matches Java accept/reject and post-db state (and result.pb field 20) - **VERIFIED**

## 2) Implement legacy exchange store routing for `ALLOW_SAME_TOKEN_NAME == 0` (only if required)

Goal: mirror `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()` behavior.

- [x] In `execute_exchange_withdraw_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] When `allow_same_token_name == 0`:
    - [x] load the exchange from v1: `storage_adapter.get_exchange_routed(exchange_id, allow_same_token_name)` routes to v1
    - [x] validate token membership against v1 token **names**
    - [x] update and persist via `put_exchange_dual_write()`:
      - [x] v1 exchange to `exchange` via `put_exchange_to_store(..., false)`
      - [x] v2 copy to `exchange-v2`:
        - [x] map token names → ids using `get_asset_issue(name, 0).id` (skip TRX `"_"`)
        - [x] store via `put_exchange_to_store(..., true)`
  - [x] When `allow_same_token_name == 1`:
    - [x] keep v2-only read + write
- [x] Add fixtures for `ALLOW_SAME_TOKEN_NAME == 0`:
  - [x] Existing `legacy_mode_happy_path_withdraw` fixture tests legacy mode
  - [x] Conformance test passes for this fixture - **VERIFIED**

Alternative (if legacy mode is explicitly out of scope):

- [N/A] When `allow_same_token_name == 0`, return a clear error that forces Java fallback (and document the limitation). **NOT NEEDED - legacy mode is now supported**

## 3) Owner-address parity (optional)

- [x] Parse `owner_address` (field 1) in `parse_exchange_withdraw_contract()` and:
  - [x] validate it matches `transaction.from` (or at least matches the derived TRON 21-byte address)
  - [x] emit Java-like errors when it doesn't (if error-string parity is desired) - **Uses hex encoding**

Note: Owner address is parsed and validated with Java-style error "account[hex] not exists".

## 4) Error string parity (optional)

- [x] Align missing-owner errors:
  - [x] replace `"Owner account not found"` with Java-style `account[<readable>] not exists` - **Now uses hex encoding**
- [x] Align creator mismatch formatting (readable address vs hex) if fixtures ever assert messages.
  - [x] Rust uses `hex::encode()` which matches Java's `ByteArray.toHexString()` (lowercase hex) - **Already correct**

## 5) Verification steps

- [x] Rust:
  - [x] `cd rust-backend && cargo test` - All 20 exchange tests pass
  - [x] run the conformance runner filtered to `exchange_withdraw_contract` fixtures - **All 12 fixtures pass**
- [ ] Java (optional, end-to-end validation):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ExchangeWithdrawActuatorTest"`

## 6) Rollout checklist

- [x] Keep `exchange_withdraw_enabled` default `false` until parity fixtures pass (or until scoped parity target is documented) - **All 12 conformance fixtures pass**
- [ ] Enable in dev/conformance environments first, then consider production configs

