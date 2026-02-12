# TODO / Fix Plan: `EXCHANGE_INJECT_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/EXCHANGE_INJECT_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [x] Confirm which modes must be supported:
  - [x] Only `ALLOW_SAME_TOKEN_NAME == 1` (modern/mainnet replay after the proposal) - **Supported**
  - [x] Must support `ALLOW_SAME_TOKEN_NAME == 0` (legacy replay) - **Supported**
  - [x] Must support `ALLOW_ASSET_OPTIMIZATION == 1` / account-asset store import semantics - **Supported**
- [x] Confirm what "parity" means operationally:
  - [x] correctness of state (exchange + account DB contents) - **Implemented**
  - [x] exact error strings - **Implemented**
  - [x] receipt bytes/fields - **Already implemented**

## 1) Fix exchange store routing (required for `ALLOW_SAME_TOKEN_NAME == 0`)

Goal: mirror `Commons.getExchangeStoreFinal()` + `Commons.putExchangeCapsule()`.

- [x] In `execute_exchange_inject_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] When `allow_same_token_name == 0`:
    - [x] load the exchange from v1: `storage_adapter.get_exchange_routed(exchange_id, allow_same_token_name)`
    - [x] validate token membership against v1 token **names**
    - [x] after updating balances, persist:
      - [x] v1 exchange to `exchange` via `put_exchange_to_store(..., false)`
      - [x] v2 copy to `exchange-v2`:
        - [x] resolve token names → ids using `get_asset_issue(name, 0).id` (skip TRX `"_"`)
        - [x] store via `put_exchange_to_store(..., true)`
  - [x] When `allow_same_token_name == 1`:
    - [x] keep the current v2-only behavior (read + write v2)

**Implementation details:**
- Added `get_exchange_routed(exchange_id, allow_same_token_name)` to `engine.rs` - routes to v1 when `allow_same_token_name == 0`, v2 otherwise
- Added `put_exchange_dual_write(exchange, allow_same_token_name)` to `engine.rs` - writes both v1 and v2 when `allow_same_token_name == 0` (with token ID transformation), v2 only otherwise
- Updated `execute_exchange_inject_contract` to use these routed methods
- Also updated `execute_exchange_withdraw_contract` and `execute_exchange_transaction_contract` for consistency

Notes:
- Java does not "backfill" v1 once allowSameTokenName==1; v1 is effectively frozen. Preserve that behavior.

## 2) Fix TRC-10 balance validation (required for legacy mode; also needed for asset optimization)

Goal: mirror `AccountCapsule.assetBalanceEnoughV2()` semantics.

- [x] Replace `storage_adapter.get_asset_balance_v2(address, token_bytes)` calls in validation with a helper that routes by `allow_same_token_name`:
  - [x] allow==0 → read `account.asset[name]`
  - [x] allow==1 → read `account.asset_v2[token_id]`
  - [x] (Optional but recommended) reuse `Self::get_asset_balance_v2(account_proto, key_bytes, allow)` already present in `mod.rs`

**Implementation details:**
- Added `get_asset_balance_routed(address, asset_key, allow_same_token_name)` to `engine.rs`
- Updated `execute_exchange_inject_contract` to use `get_asset_balance_routed` for balance checks
- Also updated `execute_exchange_transaction_contract` for consistency

- [x] If asset optimization must be supported:
  - [x] implement account-asset store lookups/import equivalent to Java's `importAsset(key)`
  - [x] update the helper to consult the account-asset store when enabled

**Implementation details:**
- `import_asset_if_optimized()` was already implemented
- Added calls to `import_asset_if_optimized()` before asset deduction/addition in:
  - `execute_exchange_inject_contract` (before `reduce_asset_amount_v2` calls)
  - `execute_exchange_withdraw_contract` (before `add_asset_amount_v2` calls)
  - `execute_exchange_transaction_contract` (before `reduce_asset_amount_v2` and `add_asset_amount_v2` calls)

## 3) Align missing-owner error string (optional but improves parity)

- [x] Change `"Owner account not found"` to Java-style:
  - [x] `account[<hex-address>] not exists`
  - [x] ensure the same address formatting used by other conformance fixtures (`StringUtil.createReadableString` → hex)

**Implementation details:**
- Updated error strings in all exchange contracts to use `format!("account[{}] not exists", hex::encode(&owner_tron))`
- Updated: `execute_exchange_inject_contract`, `execute_exchange_withdraw_contract`, `execute_exchange_transaction_contract`
- `execute_exchange_create_contract` already had the correct format

## 4) Add/extend conformance coverage (recommended)

Goal: ensure we don't regress and that legacy mode is actually validated.

- [ ] Add conformance fixtures for `ALLOW_SAME_TOKEN_NAME == 0`:
  - [ ] inject using token **names** (non-TRX/ non-TRX case, e.g. `"abc"`)
  - [ ] inject on TRX side with token-name other side (`"_"` + `"def"`)
  - [ ] assert both `exchange` and `exchange-v2` post-state matches Java expectations
- [ ] Add at least one "true happy path" success fixture that does **not** overflow in execute:
  - [ ] validate success + post-state updates
  - [ ] receipt includes `exchange_inject_another_amount`

## 5) Verification steps (before enabling in config)

- [x] Rust:
  - [x] `cd rust-backend && cargo check` - compiles successfully (warnings only)
  - [x] `cd rust-backend && cargo test` - all tests pass
  - [ ] run the conformance runner for `exchange_inject_contract` fixtures with `exchange_inject_enabled=true`
- [ ] Java (optional, if validating remote mode end-to-end):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.ExchangeInjectActuatorTest"`

## 6) Rollout checklist

- [ ] Keep `exchange_inject_enabled` default `false` until legacy-mode fixtures (if required) pass
- [ ] Enable in dev/conformance environments first, then consider production configs

---

## Summary of Changes Made

### Files Modified:

1. **`rust-backend/crates/execution/src/storage_adapter/engine.rs`**
   - Added `get_asset_balance_routed(address, asset_key, allow_same_token_name)` - routes TRC-10 balance lookup based on `allow_same_token_name` flag
   - Added `put_exchange_dual_write(exchange, allow_same_token_name)` - performs dual-write to both v1 and v2 stores when `allow_same_token_name == 0`, with token name → ID transformation
   - Added `get_exchange_routed(exchange_id, allow_same_token_name)` - routes exchange read to appropriate store

2. **`rust-backend/crates/core/src/service/mod.rs`**
   - `execute_exchange_inject_contract`:
     - Updated to use `get_exchange_routed`, `get_asset_balance_routed`, and `put_exchange_dual_write`
     - Added `import_asset_if_optimized` calls before asset deductions
     - Fixed error string to match Java format
   - `execute_exchange_withdraw_contract`:
     - Updated to use `get_exchange_routed` and `put_exchange_dual_write`
     - Added `import_asset_if_optimized` calls before asset additions
     - Fixed error string to match Java format
   - `execute_exchange_transaction_contract`:
     - Updated to use `get_exchange_routed`, `get_asset_balance_routed`, and `put_exchange_dual_write`
     - Added `import_asset_if_optimized` calls before asset deductions/additions
     - Fixed error string to match Java format

### Key Behaviors Now Matching Java:

1. **Exchange Store Routing** (`Commons.getExchangeStoreFinal`):
   - `allow_same_token_name == 0`: Reads from legacy `exchange` store (v1, token names)
   - `allow_same_token_name == 1`: Reads from `exchange-v2` store (token IDs)

2. **Exchange Dual-Write** (`Commons.putExchangeCapsule`):
   - `allow_same_token_name == 0`: Writes to BOTH stores - v1 with token names, v2 with transformed token IDs
   - `allow_same_token_name == 1`: Writes to v2 only

3. **TRC-10 Balance Validation** (`AccountCapsule.assetBalanceEnoughV2`):
   - `allow_same_token_name == 0`: Reads from `account.asset` (keyed by token name)
   - `allow_same_token_name == 1`: Reads from `account.asset_v2` (keyed by token ID)

4. **Asset Optimization** (`AccountCapsule.importAsset`):
   - When `ALLOW_ASSET_OPTIMIZATION == 1`: Imports balances from AccountAssetStore before modifications

5. **Error Strings**:
   - Account not found: `account[<hex>] not exists` (matches Java's `StringUtil.createReadableString`)
