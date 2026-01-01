# ExchangeFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/ExchangeFixtureGeneratorTest.java` fixture generation
  so conformance covers major validation branches and boundary conditions for Exchange contracts:
  - `ExchangeCreateContract` (41)
  - `ExchangeInjectContract` (42)
  - `ExchangeWithdrawContract` (43)
  - `ExchangeTransactionContract` (44)

Non-Goals
- Do not change actuator validation/execution logic; only add/adjust fixtures to reflect current behavior.
- Do not refactor fixture generator infrastructure (keep changes localized to the test class unless unavoidable).
- Do not attempt to cover legacy exchange mode (`ALLOW_SAME_TOKEN_NAME=0`) unless explicitly needed.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- Validation failures produce:
  - `metadata.json.expectedStatus == "VALIDATION_FAILED"`
  - `metadata.json.expectedErrorMessage` matches the thrown `ContractValidateException` message.
- Execution failures (reverts) produce:
  - `metadata.json.expectedStatus == "REVERT"`
  - `metadata.json.expectedErrorMessage` matches the thrown `ContractExeException` message.
- Happy fixtures execute successfully and mutate expected DBs.
- `caseCategory`/`description` remain consistent with the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines and Branches
- [x] Skim the validate paths (source of truth) and list exact error messages to match:
  - [x] `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ExchangeInjectActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ExchangeWithdrawActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ExchangeTransactionActuator.java`
- [ ] (Optional) Cross-check with unit tests for "hard to craft" branches:
  - [ ] `framework/src/test/java/org/tron/core/actuator/Exchange*ActuatorTest.java`
- [ ] Run the existing conformance generator test once to verify current fixture outcomes:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ExchangeFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — ExchangeCreateContract (41) Fixtures

Owner/address/account branches
- [x] Add `validate_fail_owner_address_invalid_empty`:
  - [x] `owner_address = ByteString.EMPTY`
  - [x] Expect error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not inserted into `AccountStore`.
  - [x] Expect error contains: `"not exists"`.

Token id format branches (ALLOW_SAME_TOKEN_NAME=1)
- [x] Add `validate_fail_first_token_id_not_number`:
  - [x] `firstTokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"first token id is not a valid number"`.
- [x] Add `validate_fail_second_token_id_not_number`:
  - [x] `secondTokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"second token id is not a valid number"`.

Balance positivity / limit branches
- [x] Add `validate_fail_first_token_balance_zero`:
  - [x] `firstTokenBalance = 0` (keep second positive)
  - [x] Expect: `"token balance must greater than zero"`.
- [x] Add `validate_fail_second_token_balance_zero`:
  - [x] `secondTokenBalance = 0`
  - [x] Expect: `"token balance must greater than zero"`.
- [x] Add `validate_fail_balance_limit_exceeded_first`:
  - [x] Set `EXCHANGE_BALANCE_LIMIT` small for this test (or set firstTokenBalance > limit).
  - [x] Expect: `"token balance must less than <limit>"`.
- [x] Add `validate_fail_balance_limit_exceeded_second`:
  - [x] secondTokenBalance > limit
  - [x] Expect: `"token balance must less than <limit>"`.

Funding branches (distinct from "insufficient fee")
- [x] Add `validate_fail_trx_side_underfunded_fee_ok`:
  - [x] Account balance is >= fee but < fee + TRX deposit amount.
  - [x] Use TRX as first/second token id.
  - [x] Expect: `"balance is not enough"`.
- [x] Add `validate_fail_first_token_balance_not_enough`:
  - [x] Use a TRC-10 token id, but seed the account with less than required.
  - [x] Expect: `"first token balance is not enough"`.
- [x] Add `validate_fail_second_token_balance_not_enough`:
  - [x] Expect: `"second token balance is not enough"`.

Phase 2 — ExchangeInjectContract (42) Fixtures

Exchange existence / owner permission
- [x] (Already present) ensure `validate_fail_nonexistent` covers the exact error message `"Exchange[<id>] not exists"`.

Token id validity / membership
- [x] Add `validate_fail_token_id_not_number`:
  - [x] `tokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"token id is not a valid number"`.
- [x] Add `validate_fail_token_id_not_in_exchange`:
  - [x] Create exchange `TRX <-> TOKEN_A`, set `tokenId = TOKEN_B`.
  - [x] Expect: `"token id is not in exchange"`.

Quant / closed-exchange branches
- [x] Add `validate_fail_zero_quant`:
  - [x] `quant = 0`
  - [x] Expect: `"injected token quant must greater than zero"`.
- [x] Add `validate_fail_exchange_closed_balance_zero`:
  - [x] Seed exchange with `firstTokenBalance=0` (or second=0).
  - [x] Expect: `"the exchange has been closed"`.

Computed proportional amount must be > 0
- [x] Add `validate_fail_calculated_another_token_quant_zero`:
  - [x] Use skewed balances (e.g., `first=10_000`, `second=1`) and `quant=1` so division truncates to 0.
  - [x] Expect: `"the calculated token quant  must be greater than 0"`.

Balance limit enforcement after injection
- [x] Add `validate_fail_balance_limit_exceeded_post_inject`:
  - [x] Set `exchangeBalanceLimit` low and create exchange near the limit.
  - [x] Inject enough to exceed it.
  - [x] Expect: `"token balance must less than <limit>"`.

Account funding failures
- [x] Add `validate_fail_inject_token_balance_not_enough`:
  - [x] Inject a token where the creator's account has less than `quant`.
  - [x] Expect: `"token balance is not enough"` (or `"balance is not enough"` when injecting TRX).
- [x] Add `validate_fail_inject_another_token_balance_not_enough`:
  - [x] Ensure the proportional other-side requirement exceeds account balance.
  - [x] Expect: `"another token balance is not enough"` (or `"balance is not enough"` if the other side is TRX).

Branch coverage: inject other side
- [x] Add `happy_path_inject_second_token_side`:
  - [x] For exchange `TRX <-> TOKEN_A`, set `tokenId = TOKEN_A` and `quant` positive.
  - [x] Expect `SUCCESS` and correct `exchangeInjectAnotherAmount` in result.pb.

Phase 3 — ExchangeWithdrawContract (43) Fixtures

Exchange existence
- [x] Add `validate_fail_nonexistent_exchange`:
  - [x] Withdraw from `exchangeId` not present.
  - [x] Expect: `"Exchange[<id>] not exists"`.

Token id validity / membership
- [x] Add `validate_fail_token_id_not_number`:
  - [x] `tokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"token id is not a valid number"`.
- [x] Add `validate_fail_token_not_in_exchange`:
  - [x] Use `TOKEN_B` for exchange `TRX <-> TOKEN_A`.
  - [x] Expect: `"token is not in exchange"`.

Quant / closed-exchange branches
- [x] Add `validate_fail_zero_quant`:
  - [x] `quant = 0`
  - [x] Expect: `"withdraw token quant must greater than zero"`.
- [x] Add `validate_fail_exchange_closed_balance_zero`:
  - [x] Seed exchange with `firstTokenBalance=0` (or second=0).
  - [x] Expect: `"the exchange has been closed"`.

Computed proportional withdrawal must be > 0
- [x] Add `validate_fail_withdraw_another_token_quant_zero`:
  - [x] Skew balances so `anotherTokenQuant` truncates to 0 for small `quant`.
  - [x] Expect: `"withdraw another token quant must greater than zero"`.

Precision guard
- [x] Add `validate_fail_not_precise_enough`:
  - [x] Use a 1:2 balance ratio and an odd withdrawal amount (mirrors unit tests):
    - [x] Example: exchange balances `(first=100_000_000, second=200_000_000)`, withdraw `secondTokenQuant=9991`.
  - [x] Expect: `"Not precise enough"`.

Branch coverage: withdraw other side
- [x] Add `happy_path_withdraw_second_token_side`:
  - [x] For exchange `TRX <-> TOKEN_A`, withdraw `TOKEN_A` instead of TRX.
  - [x] Expect `SUCCESS` and correct `exchangeWithdrawAnotherAmount` in result.pb.

Phase 4 — ExchangeTransactionContract (44) Fixtures

Exchange existence
- [x] Add `validate_fail_nonexistent_exchange`:
  - [x] `exchangeId` not present.
  - [x] Expect: `"Exchange[<id>] not exists"`.

Token id validity / membership
- [x] Add `validate_fail_token_id_not_number`:
  - [x] `tokenId = "abc".getBytes()` (non-TRX)
  - [x] Expect: `"token id is not a valid number"`.

Expected lower-bound checks
- [x] Add `validate_fail_expected_zero`:
  - [x] `expected = 0`
  - [x] Expect: `"token expected must greater than zero"`.

Closed exchange
- [x] Add `validate_fail_exchange_closed_balance_zero`:
  - [x] Set exchange first or second balance to 0.
  - [x] Expect: `"the exchange has been closed"`.

Balance limit enforcement
- [x] Add `validate_fail_balance_limit_exceeded_selected_side`:
  - [x] Set `exchangeBalanceLimit` low and choose `quant` that pushes the selected side over the limit.
  - [x] Expect: `"token balance must less than <limit>"`.

Account funding failures
- [x] Add `validate_fail_trx_balance_not_enough`:
  - [x] Trade with `tokenId=TRX` but account TRX balance < `quant`.
  - [x] Expect: `"balance is not enough"`.
- [x] Add `validate_fail_token_balance_not_enough`:
  - [x] Trade with `tokenId=TOKEN_A` but token balance < `quant`.
  - [x] Expect: `"token balance is not enough"`.

Behavioral: permissionless trading
- [x] Add `happy_path_non_creator_can_trade`:
  - [x] Use `OTHER_ADDRESS` to trade against an exchange created by `OWNER_ADDRESS`.
  - [x] Ensure `OTHER_ADDRESS` is funded with the sold token.
  - [x] Expect `SUCCESS`.

Optional: strict math divergence
- [ ] Add `happy_path_strict_math_enabled` (optional, only if the conformance harness cares):
  - [ ] Set dynamic property `allowStrictMath=true` and regenerate a known trade.
  - [ ] Compare output amounts vs the default fixture to confirm it actually changes.

Phase 5 — Hygiene and Consistency
- [x] Ensure each fixture lists the correct databases in `FixtureMetadata`:
  - [x] `account`, `exchange-v2`, `dynamic-properties` (and `asset-issue-v2` only if mutated/needed).
- [x] Use unique `exchangeId` per test or clear `ExchangeV2Store` between tests to avoid state bleed.
- [x] Keep `ALLOW_SAME_TOKEN_NAME=1` explicit per test if any test temporarily changes it.
- [x] If time-sensitive fields matter (e.g., exchange `createTime`), consider aligning
      `DynamicPropertiesStore.saveLatestBlockHeaderTimestamp(...)` with the block context used in metadata.
- [ ] Re-run only the single test class to regenerate fixtures and spot-check a few `metadata.json` files:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ExchangeFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

