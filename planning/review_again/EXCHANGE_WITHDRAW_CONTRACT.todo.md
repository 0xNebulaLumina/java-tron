# TODO / Fix Plan: `EXCHANGE_WITHDRAW_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_WITHDRAW_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm which modes must be supported:
  - [ ] Only `ALLOW_SAME_TOKEN_NAME == 1` (modern/mainnet)
  - [ ] Must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay; exchange store + name keys)
- [ ] Confirm what “parity” means operationally:
  - [ ] correctness of state (exchange + account DB contents)
  - [ ] exact acceptance/rejection boundaries (esp. “Not precise enough”)
  - [ ] exact error strings
  - [ ] receipt bytes/fields

## 1) Fix the “Not precise enough” precision check (high priority if strict parity matters)

Goal: mirror Java’s `BigDecimal.divide(..., 4, ROUND_HALF_UP)` based check from `ExchangeWithdrawActuator.validate`.

- [ ] Update `is_withdraw_precise_enough()` in `rust-backend/crates/core/src/service/contracts/exchange.rs`:
  - [ ] Stop using raw `f64` division for this check.
  - [ ] Implement an integer math equivalent of Java’s 4dp half-up rounding:
    - [ ] `another = floor(numerator / denom)` where `numerator = other_balance * token_quant`
    - [ ] `q4_scaled = round_half_up((numerator * 10000) / denom)` (integer result; represents `q4 * 10000`)
    - [ ] `remainder_scaled = q4_scaled - (another * 10000)`
    - [ ] Java rejects if `(remainder / another) > 0.0001` → in scaled integers this is `remainder_scaled > another`
  - [ ] Confirm sign behavior is correct (today all inputs are positive, but implementation should be robust).
- [ ] Add unit tests in `rust-backend/crates/core/src/service/contracts/exchange.rs`:
  - [ ] boundary cases where Java passes but float-exact would fail:
    - [ ] `another == 1` and true remainder in `(0.0001, 0.00015)` should **pass** with Java rounding
    - [ ] true remainder ≥ `0.00015` should **fail** (rounds to `0.0002`)
  - [ ] cases where `another >= 10000` should always pass (since the ratio threshold becomes ≥ 1.0)
- [ ] Add a conformance fixture that exercises the boundary:
  - [ ] Craft pre-state balances so that:
    - [ ] `anotherTokenQuant` is small (<10000)
    - [ ] exact remainder sits in the “rounds down to 0.0001” band
  - [ ] Assert Rust matches Java accept/reject and post-db state (and result.pb field 20).

## 2) Implement legacy exchange store routing for `ALLOW_SAME_TOKEN_NAME == 0` (only if required)

Goal: mirror `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()` behavior.

- [ ] In `execute_exchange_withdraw_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] When `allow_same_token_name == 0`:
    - [ ] load the exchange from v1: `storage_adapter.get_exchange_from_store(exchange_id, false)`
    - [ ] validate token membership against v1 token **names**
    - [ ] update and persist:
      - [ ] v1 exchange to `exchange` via `put_exchange_to_store(..., false)`
      - [ ] v2 copy to `exchange-v2`:
        - [ ] map token names → ids using `get_asset_issue(name, 0).id` (skip TRX `"_"`)
        - [ ] store via `put_exchange_to_store(..., true)`
  - [ ] When `allow_same_token_name == 1`:
    - [ ] keep v2-only read + write
- [ ] Add fixtures for `ALLOW_SAME_TOKEN_NAME == 0`:
  - [ ] withdraw from a name-keyed exchange (non-TRX/non-TRX)
  - [ ] withdraw on the TRX side of a name-keyed exchange
  - [ ] assert both `exchange` and `exchange-v2` post-state matches Java expectations

Alternative (if legacy mode is explicitly out of scope):

- [ ] When `allow_same_token_name == 0`, return a clear error that forces Java fallback (and document the limitation).

## 3) Owner-address parity (optional)

- [ ] Parse `owner_address` (field 1) in `parse_exchange_withdraw_contract()` and:
  - [ ] validate it matches `transaction.from` (or at least matches the derived TRON 21-byte address)
  - [ ] emit Java-like errors when it doesn’t (if error-string parity is desired)

## 4) Error string parity (optional)

- [ ] Align missing-owner errors:
  - [ ] replace `"Owner account not found"` with Java-style `account[<readable>] not exists`
- [ ] Align creator mismatch formatting (readable address vs hex) if fixtures ever assert messages.

## 5) Verification steps

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] run the conformance runner filtered to `exchange_withdraw_contract` fixtures
- [ ] Java (optional, end-to-end validation):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ExchangeWithdrawActuatorTest"`

## 6) Rollout checklist

- [ ] Keep `exchange_withdraw_enabled` default `false` until parity fixtures pass (or until scoped parity target is documented)
- [ ] Enable in dev/conformance environments first, then consider production configs

