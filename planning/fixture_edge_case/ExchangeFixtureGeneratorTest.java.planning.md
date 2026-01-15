Review Target

- `framework/src/test/java/org/tron/core/conformance/ExchangeFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `ExchangeCreateContract` (type 41)
  - `ExchangeInjectContract` (type 42)
  - `ExchangeWithdrawContract` (type 43)
  - `ExchangeTransactionContract` (type 44)
- Baseline mode in this test class:
  - `ALLOW_SAME_TOKEN_NAME = 1` (V2 id-based exchanges; `exchange-v2` store)
  - `EXCHANGE_CREATE_FEE = 1024_000_000` and `EXCHANGE_BALANCE_LIMIT = 100_000_000_000_000_000`
  - Fees: 41 charges `exchangeCreateFee`; 42/43/44 `calcFee()` is `0` in Java-tron

Current Coverage (as written)

ExchangeCreateContract (41)

- Happy: create exchange `TRX <-> TOKEN_A`.
- Happy: create exchange `TOKEN_A <-> TOKEN_B`.
- Validate-fail: insufficient TRX for create fee.
- Validate-fail: same token on both sides.

ExchangeInjectContract (42)

- Happy: creator injects additional liquidity (injecting `TRX` side).
- Validate-fail: non-creator inject attempt.
- Validate-fail: exchange does not exist.

ExchangeWithdrawContract (43)

- Happy: creator withdraws liquidity (withdrawing `TRX` side).
- Validate-fail: non-creator withdraw attempt.
- Validate-fail: withdraw amount exceeds exchange balance.

ExchangeTransactionContract (44)

- Happy: swap selling `TRX` for `TOKEN_A`.
- Happy: swap selling `TOKEN_A` for `TRX` (reverse direction).
- Validate-fail: expected output too high (slippage check).
- Validate-fail: token not in exchange.
- Validate-fail: `quant == 0`.

Missing Edge Cases (high value for conformance)

Validation paths:
- `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeInjectActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeWithdrawActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeTransactionActuator.java`

Common (applies to multiple contracts)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`): empty / wrong length / wrong prefix bytes.
- Owner account does not exist (`AccountStore.has(ownerAddress) == false`).
- Invalid token id encoding when `ALLOW_SAME_TOKEN_NAME=1`:
  - non-numeric token id bytes (e.g. `"abc"`) for any non-TRX token.
- Closed exchange handling for 42/43/44:
  - `firstTokenBalance == 0` or `secondTokenBalance == 0` yields
    `"Token balance in exchange is equal with 0,the exchange has been closed"`.

ExchangeCreateContract (41) — missing validation branches

- Token id numeric checks (only in `ALLOW_SAME_TOKEN_NAME=1` mode):
  - first token invalid: `"first token id is not a valid number"`.
  - second token invalid: `"second token id is not a valid number"`.
- Token balances must be positive:
  - `firstTokenBalance <= 0` or `secondTokenBalance <= 0`
    → `"token balance must greater than zero"`.
- Token balances must be within `exchangeBalanceLimit`:
  - either side > limit → `"token balance must less than <limit>"`.
- Funding checks beyond “insufficient fee”:
  - TRX side underfunded even though fee is covered
    → `"balance is not enough"`.
  - Insufficient TRC-10 V2 balance:
    - first token insufficient → `"first token balance is not enough"`.
    - second token insufficient → `"second token balance is not enough"`.

ExchangeInjectContract (42) — missing validation branches + branch coverage

- Token id must belong to the exchange:
  - `"token id is not in exchange"` (distinct from 44/43 messaging).
- `quant <= 0`:
  - `"injected token quant must greater than zero"`.
- Computed proportional amount must be > 0:
  - `"the calculated token quant  must be greater than 0"` (requires skewed balances + small `quant`).
- Balance limit enforcement after inject:
  - post-inject balance exceeds `exchangeBalanceLimit`
    → `"token balance must less than <limit>"`.
- Account funding failures:
  - injected token insufficient:
    - TRX → `"balance is not enough"`
    - token → `"token balance is not enough"`
  - “other side” insufficient to satisfy proportional requirement:
    - TRX → `"balance is not enough"`
    - token → `"another token balance is not enough"`
- Happy-path injecting the *second* token side (covers the opposite calc/execute branch).

ExchangeWithdrawContract (43) — missing validation branches + branch coverage

- Exchange does not exist:
  - `"Exchange[<id>] not exists"` (no conformance fixture currently exercises this).
- Token id must belong to the exchange:
  - `"token is not in exchange"`.
- `quant <= 0`:
  - `"withdraw token quant must greater than zero"`.
- Computed proportional withdrawal must be > 0:
  - `"withdraw another token quant must greater than zero"`.
- Precision guard:
  - `"Not precise enough"` (important rounding/precision behavior; easy to regress cross-impl).
- Happy-path withdrawing the *second* token side (covers the opposite calc/execute branch).

ExchangeTransactionContract (44) — missing validation branches + behavioral coverage

- Exchange does not exist:
  - `"Exchange[<id>] not exists"`.
- `expected <= 0`:
  - `"token expected must greater than zero"`.
- Balance limit enforcement:
  - selected side balance + `quant` exceeds `exchangeBalanceLimit`
    → `"token balance must less than <limit>"`.
- Account funding failures:
  - TRX → `"balance is not enough"`
  - token → `"token balance is not enough"`.
- Happy-path trade by a *non-creator* account:
  - trading is permissionless (unlike inject/withdraw); a fixture should lock this in.
- Optional: strict-math mode difference:
  - `exchangeCapsule.transaction(..., dynamicStore.allowStrictMath())` can change outcomes;
    no fixture covers `allowStrictMath=true/false` divergence.

Notes / Potential fixture-generation pitfalls

- `createTransaction()` / `createBlockContext()` are local helpers using `System.currentTimeMillis()`;
  other conformance generator tests often use `ConformanceFixtureTestSupport` for deterministic raw data.
- `FixtureGenerator.executeEmbedded()` does not apply `blockCap` into `DynamicPropertiesStore` before
  actuator execution; actuators read head block time/number from `DynamicPropertiesStore`, so time-sensitive
  state (e.g., exchange `createTime`) can diverge from fixture `metadata.json` block timestamp unless
  tests explicitly keep them aligned.
- `FixtureGenerator` derives `expectedStatus/expectedErrorMessage` from actual execution. If a fixture
  doesn’t hit the intended validation branch, the produced `metadata.json` will contradict the test’s
  `caseCategory`/description.

