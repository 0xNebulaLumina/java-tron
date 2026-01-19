# Review: `EXCHANGE_WITHDRAW_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**
  - `execute_exchange_withdraw_contract()` + `parse_exchange_withdraw_contract()` in `rust-backend/crates/core/src/service/mod.rs`
  - math helpers in `rust-backend/crates/core/src/service/contracts/exchange.rs`:
    - `calculate_withdraw_another_amount()`
    - `is_withdraw_precise_enough()`
  - exchange store helpers in `rust-backend/crates/execution/src/storage_adapter/engine.rs`:
    - `get_exchange{,_from_store}()`, `put_exchange{,_to_store}()`
- **Java reference**
  - `ExchangeWithdrawActuator.validate/execute` in `actuator/src/main/java/org/tron/core/actuator/ExchangeWithdrawActuator.java`
  - exchange store routing + dual-write behavior in `chainbase/src/main/java/org/tron/common/utils/Commons.java`:
    - `getExchangeStoreFinal()`, `putExchangeCapsule()`
  - TRC-10 balance update behavior in `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`:
    - `addAssetAmountV2()`

Goal: determine whether Rust execution matches java-tron’s actuator semantics (validation, state updates, and receipt fields), and call out any parity gaps.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`ExchangeWithdrawActuator.validate`)

Key checks (in order):

1. Contract Any type is `ExchangeWithdrawContract`
2. `owner_address` is a valid TRON address (`DecodeUtil.addressValid`) and account exists
3. Account has enough TRX balance to pay the fee (`calcFee() == 0` today, so this always passes)
4. Exchange exists, fetched via:
   - `Commons.getExchangeStoreFinal(dynamicStore, exchangeStore, exchangeV2Store)`
   - `ALLOW_SAME_TOKEN_NAME == 0` → `ExchangeStore` (“v1”, token **names**)
   - `ALLOW_SAME_TOKEN_NAME == 1` → `ExchangeV2Store` (“v2”, token **ids**)
5. Owner is the exchange creator
6. When `ALLOW_SAME_TOKEN_NAME == 1` and token != TRX (`"_"`), token id must be numeric (`TransactionUtil.isNumber`)
7. `token_id` must be one side of the exchange
8. `quant > 0`
9. Exchange must not be closed (`firstBalance != 0 && secondBalance != 0`)
10. Compute proportional other-side withdrawal amount using **BigDecimal floor**:
    - `another = floor(otherBalance * quant / tokenBalance)`
11. Exchange balances must be sufficient (`tokenBalance >= quant` and `otherBalance >= another`)
12. `another > 0`
13. **Precision check** (“Not precise enough”):
    - Compute a decimal quotient rounded to **4 decimal places**, **half-up**:
      - `q4 = round_half_up((otherBalance * quant) / tokenBalance, scale=4)`
    - `remainder = q4 - another`
    - Fail if `remainder / another > 0.0001`

Important nuance: the precision check is not based on the exact rational value; it’s explicitly computed via `BigDecimal.divide(..., 4, ROUND_HALF_UP)`.

### 2) Execution (`ExchangeWithdrawActuator.execute`)

- Computes `anotherTokenQuant` using **BigInteger**:
  - `another = (otherBalance * quant) / tokenBalance` (integer division; no `multiplyExact` overflow behavior here)
- Updates:
  - exchange balances: subtract `quant` from the chosen side and subtract `another` from the other side
  - creator’s holdings: add `quant` (TRX or TRC-10) and add `another` (TRX or TRC-10)
  - store exchange via `Commons.putExchangeCapsule()`:
    - if `ALLOW_SAME_TOKEN_NAME == 0`: write v1 exchange (names) **and** write a v2 copy (ids)
    - if `ALLOW_SAME_TOKEN_NAME == 1`: write v2 only
- Receipt: `ret.setExchangeWithdrawAnotherAmount(another)` + `ret.setStatus(fee=0, SUCESS)`

---

## Rust implementation behavior (what it currently does)

### Parsing

`parse_exchange_withdraw_contract()` is implemented by calling `parse_exchange_inject_contract()` and reading:

- `exchange_id` (field 2, int64)
- `token_id` (field 3, bytes)
- `quant` (field 4, int64)

(`owner_address` is skipped and the sender is taken from `transaction.from`.)

### Validation + execution

`execute_exchange_withdraw_contract()`:

- loads the owner account (by `transaction.from`)
- loads `ALLOW_SAME_TOKEN_NAME`
- fetches the exchange via `storage_adapter.get_exchange(exchange_id)` (always **v2**)
- validates:
  - creator check
  - numeric token-id format when `ALLOW_SAME_TOKEN_NAME == 1`
  - token membership in the exchange
  - non-closed exchange and positive quant
  - calculates `another_token_quant` via `calculate_withdraw_another_amount()` (i128 multiply then `/` → integer floor)
  - checks exchange balance sufficiency via `new_first_balance < 0 || new_second_balance < 0`
  - checks `another_token_quant > 0`
  - precision check via `is_withdraw_precise_enough()` (currently **f64-based**, no 4dp half-up rounding)
- executes:
  - adds withdrawn token amounts to the creator’s TRX balance or TRC-10 balances via `add_asset_amount_v2()`
  - updates the exchange via `storage_adapter.put_exchange()` (writes **v2 only**)
  - builds a receipt with `exchange_withdraw_another_amount` set

---

## Does it match java-tron?

### What matches (good parity)

1) **Core ratio math for `anotherTokenQuant` matches**

- Java execute uses BigInteger integer division: `floor(otherBalance * quant / tokenBalance)`.
- Rust uses i128 integer math and truncating division for the same floor result.

2) **Main validation checks match in modern mode (`ALLOW_SAME_TOKEN_NAME == 1`)**

- creator-only restriction
- numeric token-id enforcement (non-TRX) matches `TransactionUtil.isNumber`
- token-in-exchange, quant>0, non-closed exchange
- failure strings for the common branches are intentionally identical (e.g. “Not precise enough”)

3) **State updates match for the exchange-v2 store**

- Both systems update the exchange’s two balances by subtracting `quant` on one side and the computed `another` on the other.
- Both credit the owner with both token amounts (TRX balance and/or TRC-10 balances).

4) **Receipt field parity matches existing fixtures**

The conformance fixtures expect only the `exchange_withdraw_another_amount` field (Transaction.Result field 20) to be set (example: `conformance/fixtures/exchange_withdraw_contract/happy_path_withdraw/expected/result.pb` decodes to only `20: ...`), and Rust does set this field.

### Where it diverges (real parity breaks / risk areas)

1) **Precision check semantics do not match Java**

Java uses:

- `q4 = BigDecimal(...).divide(..., 4, ROUND_HALF_UP)` (quantized to 1e-4)
- `remainder = q4 - floor(...)`
- reject if `remainder / floor > 0.0001` (strict `>` in Java)

Rust uses:

- `exact = (other_balance as f64) * (token_quant as f64) / (token_balance as f64)` (not rounded to 4 decimals)
- `remainder = exact - floor(...)`
- accept if `remainder / floor <= 0.0001`

Impact:

- Rust is **strictly different** for borderline cases where the true remainder is in `(0.0001, 0.00015)` (for `another==1`) and Java’s 4dp half-up rounding would quantize it down to `0.0001` (passing), while Rust would reject it.
- Even when not at the boundary, Rust is not implementing the same “4-decimal-place half-up” logic that Java explicitly uses.

If the parity target includes exact Java acceptance/rejection, `is_withdraw_precise_enough()` must be reworked to match Java’s 4dp rounding semantics (ideally via integer math, not floats).

2) **`ALLOW_SAME_TOKEN_NAME == 0` (legacy) routing is not implemented**

Java routes to `ExchangeStore` (v1) when `ALLOW_SAME_TOKEN_NAME == 0`, where exchange token keys are **names**, and writes **both** v1 and v2 on update (`Commons.putExchangeCapsule`).

Rust always:

- reads from v2 (`get_exchange`)
- writes v2 only (`put_exchange`)

Impact:

- On legacy-mode chains/blocks, contracts that use token **names** will fail membership checks against v2 exchanges that store token **ids** (because Java’s `resetTokenWithID()` converts names → ids when populating v2).
- Even if execution succeeded, v1 would be left stale while Java expects v1+v2 updates in allowSameTokenName==0 mode.

3) **Owner-address field is ignored (likely acceptable, but not exact parity)**

Java validates `owner_address` from the contract. Rust skips it and uses `transaction.from`. If the broader pipeline guarantees signer/owner alignment before remote execution, this is fine; but it’s not a byte-for-byte equivalent validator.

4) **Minor: missing-owner / creator error-string formatting**

Examples:

- missing owner: Rust returns `"Owner account not found"`; Java emits `account[...] not exists`
- not creator: Java formats a readable address; Rust uses hex encoding

Existing fixtures don’t appear to assert these strings, but it’s still a parity difference if exact error text matters.

---

## Bottom line

- **Mostly yes (for modern/mainnet mode)**: the Rust `EXCHANGE_WITHDRAW_CONTRACT` implementation matches Java’s core state transition logic when `ALLOW_SAME_TOKEN_NAME == 1` and values are away from the “Not precise enough” boundary.
- **No (for strict behavioral parity)**:
  - the precision check is not implemented the same way (Java’s 4dp half-up `BigDecimal` vs Rust’s unrounded `f64`)
  - legacy mode (`ALLOW_SAME_TOKEN_NAME == 0`) store routing + dual-write behavior is missing

If we care about full parity, the fixes are straightforward and mostly localized (see `planning/review_again/EXCHANGE_WITHDRAW_CONTRACT.todo.md`).

