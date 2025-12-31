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
- `caseCategory`/`description` remain consistent with the observed result (avoid “validate_fail but SUCCESS”).

Checklist / TODO

Phase 0 — Confirm Baselines and Branches
- [ ] Skim the validate paths (source of truth) and list exact error messages to match:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/ExchangeInjectActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/ExchangeWithdrawActuator.java`
  - [ ] `actuator/src/main/java/org/tron/core/actuator/ExchangeTransactionActuator.java`
- [ ] (Optional) Cross-check with unit tests for “hard to craft” branches:
  - [ ] `framework/src/test/java/org/tron/core/actuator/Exchange*ActuatorTest.java`
- [ ] Run the existing conformance generator test once to verify current fixture outcomes:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ExchangeFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

Phase 1 — ExchangeCreateContract (41) Fixtures

Owner/address/account branches
- [ ] Add `validate_fail_owner_address_invalid_empty`:
  - [ ] `owner_address = ByteString.EMPTY`
  - [ ] Expect error: `"Invalid address"`.
- [ ] Add `validate_fail_owner_account_not_exist`:
  - [ ] Use a valid-looking address not inserted into `AccountStore`.
  - [ ] Expect error contains: `"not exists"`.

Token id format branches (ALLOW_SAME_TOKEN_NAME=1)
- [ ] Add `validate_fail_first_token_id_not_number`:
  - [ ] `firstTokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"first token id is not a valid number"`.
- [ ] Add `validate_fail_second_token_id_not_number`:
  - [ ] `secondTokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"second token id is not a valid number"`.

Balance positivity / limit branches
- [ ] Add `validate_fail_first_token_balance_zero`:
  - [ ] `firstTokenBalance = 0` (keep second positive)
  - [ ] Expect: `"token balance must greater than zero"`.
- [ ] Add `validate_fail_second_token_balance_zero`:
  - [ ] `secondTokenBalance = 0`
  - [ ] Expect: `"token balance must greater than zero"`.
- [ ] Add `validate_fail_balance_limit_exceeded_first`:
  - [ ] Set `EXCHANGE_BALANCE_LIMIT` small for this test (or set firstTokenBalance > limit).
  - [ ] Expect: `"token balance must less than <limit>"`.
- [ ] Add `validate_fail_balance_limit_exceeded_second`:
  - [ ] secondTokenBalance > limit
  - [ ] Expect: `"token balance must less than <limit>"`.

Funding branches (distinct from “insufficient fee”)
- [ ] Add `validate_fail_trx_side_underfunded_fee_ok`:
  - [ ] Account balance is >= fee but < fee + TRX deposit amount.
  - [ ] Use TRX as first/second token id.
  - [ ] Expect: `"balance is not enough"`.
- [ ] Add `validate_fail_first_token_balance_not_enough`:
  - [ ] Use a TRC-10 token id, but seed the account with less than required.
  - [ ] Expect: `"first token balance is not enough"`.
- [ ] Add `validate_fail_second_token_balance_not_enough`:
  - [ ] Expect: `"second token balance is not enough"`.

Phase 2 — ExchangeInjectContract (42) Fixtures

Exchange existence / owner permission
- [ ] (Already present) ensure `validate_fail_nonexistent` covers the exact error message `"Exchange[<id>] not exists"`.

Token id validity / membership
- [ ] Add `validate_fail_token_id_not_number`:
  - [ ] `tokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"token id is not a valid number"`.
- [ ] Add `validate_fail_token_id_not_in_exchange`:
  - [ ] Create exchange `TRX <-> TOKEN_A`, set `tokenId = TOKEN_B`.
  - [ ] Expect: `"token id is not in exchange"`.

Quant / closed-exchange branches
- [ ] Add `validate_fail_zero_quant`:
  - [ ] `quant = 0`
  - [ ] Expect: `"injected token quant must greater than zero"`.
- [ ] Add `validate_fail_exchange_closed_balance_zero`:
  - [ ] Seed exchange with `firstTokenBalance=0` (or second=0).
  - [ ] Expect: `"the exchange has been closed"`.

Computed proportional amount must be > 0
- [ ] Add `validate_fail_calculated_another_token_quant_zero`:
  - [ ] Use skewed balances (e.g., `first=10_000`, `second=1`) and `quant=1` so division truncates to 0.
  - [ ] Expect: `"the calculated token quant  must be greater than 0"`.

Balance limit enforcement after injection
- [ ] Add `validate_fail_balance_limit_exceeded_post_inject`:
  - [ ] Set `exchangeBalanceLimit` low and create exchange near the limit.
  - [ ] Inject enough to exceed it.
  - [ ] Expect: `"token balance must less than <limit>"`.

Account funding failures
- [ ] Add `validate_fail_inject_token_balance_not_enough`:
  - [ ] Inject a token where the creator’s account has less than `quant`.
  - [ ] Expect: `"token balance is not enough"` (or `"balance is not enough"` when injecting TRX).
- [ ] Add `validate_fail_inject_another_token_balance_not_enough`:
  - [ ] Ensure the proportional other-side requirement exceeds account balance.
  - [ ] Expect: `"another token balance is not enough"` (or `"balance is not enough"` if the other side is TRX).

Branch coverage: inject other side
- [ ] Add `happy_path_inject_second_token_side`:
  - [ ] For exchange `TRX <-> TOKEN_A`, set `tokenId = TOKEN_A` and `quant` positive.
  - [ ] Expect `SUCCESS` and correct `exchangeInjectAnotherAmount` in result.pb.

Phase 3 — ExchangeWithdrawContract (43) Fixtures

Exchange existence
- [ ] Add `validate_fail_nonexistent_exchange`:
  - [ ] Withdraw from `exchangeId` not present.
  - [ ] Expect: `"Exchange[<id>] not exists"`.

Token id validity / membership
- [ ] Add `validate_fail_token_id_not_number`:
  - [ ] `tokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"token id is not a valid number"`.
- [ ] Add `validate_fail_token_not_in_exchange`:
  - [ ] Use `TOKEN_B` for exchange `TRX <-> TOKEN_A`.
  - [ ] Expect: `"token is not in exchange"`.

Quant / closed-exchange branches
- [ ] Add `validate_fail_zero_quant`:
  - [ ] `quant = 0`
  - [ ] Expect: `"withdraw token quant must greater than zero"`.
- [ ] Add `validate_fail_exchange_closed_balance_zero`:
  - [ ] Seed exchange with `firstTokenBalance=0` (or second=0).
  - [ ] Expect: `"the exchange has been closed"`.

Computed proportional withdrawal must be > 0
- [ ] Add `validate_fail_withdraw_another_token_quant_zero`:
  - [ ] Skew balances so `anotherTokenQuant` truncates to 0 for small `quant`.
  - [ ] Expect: `"withdraw another token quant must greater than zero"`.

Precision guard
- [ ] Add `validate_fail_not_precise_enough`:
  - [ ] Use a 1:2 balance ratio and an odd withdrawal amount (mirrors unit tests):
    - [ ] Example: exchange balances `(first=100_000_000, second=200_000_000)`, withdraw `secondTokenQuant=9991`.
  - [ ] Expect: `"Not precise enough"`.

Branch coverage: withdraw other side
- [ ] Add `happy_path_withdraw_second_token_side`:
  - [ ] For exchange `TRX <-> TOKEN_A`, withdraw `TOKEN_A` instead of TRX.
  - [ ] Expect `SUCCESS` and correct `exchangeWithdrawAnotherAmount` in result.pb.

Phase 4 — ExchangeTransactionContract (44) Fixtures

Exchange existence
- [ ] Add `validate_fail_nonexistent_exchange`:
  - [ ] `exchangeId` not present.
  - [ ] Expect: `"Exchange[<id>] not exists"`.

Token id validity / membership
- [ ] Add `validate_fail_token_id_not_number`:
  - [ ] `tokenId = "abc".getBytes()` (non-TRX)
  - [ ] Expect: `"token id is not a valid number"`.

Expected lower-bound checks
- [ ] Add `validate_fail_expected_zero`:
  - [ ] `expected = 0`
  - [ ] Expect: `"token expected must greater than zero"`.

Closed exchange
- [ ] Add `validate_fail_exchange_closed_balance_zero`:
  - [ ] Set exchange first or second balance to 0.
  - [ ] Expect: `"the exchange has been closed"`.

Balance limit enforcement
- [ ] Add `validate_fail_balance_limit_exceeded_selected_side`:
  - [ ] Set `exchangeBalanceLimit` low and choose `quant` that pushes the selected side over the limit.
  - [ ] Expect: `"token balance must less than <limit>"`.

Account funding failures
- [ ] Add `validate_fail_trx_balance_not_enough`:
  - [ ] Trade with `tokenId=TRX` but account TRX balance < `quant`.
  - [ ] Expect: `"balance is not enough"`.
- [ ] Add `validate_fail_token_balance_not_enough`:
  - [ ] Trade with `tokenId=TOKEN_A` but token balance < `quant`.
  - [ ] Expect: `"token balance is not enough"`.

Behavioral: permissionless trading
- [ ] Add `happy_path_non_creator_can_trade`:
  - [ ] Use `OTHER_ADDRESS` to trade against an exchange created by `OWNER_ADDRESS`.
  - [ ] Ensure `OTHER_ADDRESS` is funded with the sold token.
  - [ ] Expect `SUCCESS`.

Optional: strict math divergence
- [ ] Add `happy_path_strict_math_enabled` (optional, only if the conformance harness cares):
  - [ ] Set dynamic property `allowStrictMath=true` and regenerate a known trade.
  - [ ] Compare output amounts vs the default fixture to confirm it actually changes.

Phase 5 — Hygiene and Consistency
- [ ] Ensure each fixture lists the correct databases in `FixtureMetadata`:
  - [ ] `account`, `exchange-v2`, `dynamic-properties` (and `asset-issue-v2` only if mutated/needed).
- [ ] Use unique `exchangeId` per test or clear `ExchangeV2Store` between tests to avoid state bleed.
- [ ] Keep `ALLOW_SAME_TOKEN_NAME=1` explicit per test if any test temporarily changes it.
- [ ] If time-sensitive fields matter (e.g., exchange `createTime`), consider aligning
      `DynamicPropertiesStore.saveLatestBlockHeaderTimestamp(...)` with the block context used in metadata.
- [ ] Re-run only the single test class to regenerate fixtures and spot-check a few `metadata.json` files:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ExchangeFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`

