Review Target

- `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`

Scope

- Fixture generation for Resource/Delegation + V2 unfreeze lifecycle:
  - `WithdrawExpireUnfreezeContract` (56)
  - `DelegateResourceContract` (57)
  - `UnDelegateResourceContract` (58)
  - `CancelAllUnfreezeV2Contract` (59)
- Baseline flags/state seeded by this test class:
  - `UNFREEZE_DELAY_DAYS = 14` (enables unfreeze-delay features; `supportUnfreezeDelay() == true`)
  - `ALLOW_DELEGATE_RESOURCE = 1` (enables `supportDR() == true`)
  - `ALLOW_NEW_RESOURCE_MODEL = 1`
  - `ALLOW_CANCEL_ALL_UNFREEZE_V2 = 1` (enables `supportAllowCancelAllUnfreezeV2() == true`)
  - Total weights set to `1_000_000_000` for NET/ENERGY/TRON_POWER
  - Head block timestamp/number seeded to `System.currentTimeMillis()` / `10`

Current Coverage (as written)

WithdrawExpireUnfreezeContract (56)

- Happy: withdraw a single expired `unfrozenV2` entry (BANDWIDTH).
- Validate-fail: no `unfrozenV2` entries (“nothing to withdraw”).
- Validate-fail: only unexpired `unfrozenV2` entries (“not yet expired”).
- Happy: multiple expired entries across BANDWIDTH + ENERGY.

DelegateResourceContract (57)

- Happy: delegate BANDWIDTH.
- Happy: delegate ENERGY.
- Happy: delegate with `lock=true` (note: `lockPeriod` may be ignored unless max-lock feature is enabled).
- Validate-fail: owner has no frozenV2 balance (insufficient available).
- Validate-fail: owner delegates to self.

UnDelegateResourceContract (58)

- Happy: undelegate a partial amount (BANDWIDTH).
- Validate-fail: no delegation exists.
- Validate-fail: undelegate amount exceeds delegated amount.

CancelAllUnfreezeV2Contract (59)

- Happy: cancel a single unexpired `unfrozenV2` entry.
- Happy: cancel mixed expired + unexpired entries (expired withdrawn, unexpired re-frozen).
- Validate-fail: no `unfrozenV2` entries to cancel.
- Validate-fail: feature disabled (`ALLOW_CANCEL_ALL_UNFREEZE_V2 = 0`).

Missing Edge Cases (high value for conformance)

Validation paths (source of truth)

- `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`
- Delegation unlock boundary semantics:
  - `chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java` (`unLockExpireResource` uses strict `< now`)

WithdrawExpireUnfreezeContract (56) — missing branches / boundaries

- Feature gating: validate-fail when `supportUnfreezeDelay() == false` (e.g. `unfreezeDelayDays == 0`) with:
  - `"Not support WithdrawExpireUnfreeze transaction, need to be opened by the committee"`.
- Address/account validation:
  - invalid `ownerAddress` → `"Invalid address"`.
  - owner account does not exist → `"Account[...] not exists"`.
- Time/list boundaries:
  - Mixed expired + unexpired entries: should succeed, withdraw only expired amounts, and keep unexpired entries.
  - Expiry boundary: `unfreezeExpireTime == now` is considered expired (`<= now`) and should be withdrawable.
- Overflow protection:
  - `LongMath.checkedAdd(balance, withdrawAmount)` throws → validate-fail with the `ArithmeticException` message.

DelegateResourceContract (57) — missing branches / boundaries

- Feature gating:
  - validate-fail when `supportDR() == false` (e.g. `ALLOW_DELEGATE_RESOURCE = 0`) → `"No support for resource delegate"`.
  - validate-fail when `supportUnfreezeDelay() == false` (e.g. `unfreezeDelayDays == 0`) → `"Not support Delegate resource transaction, need to be opened by the committee"`.
- Address/account validation:
  - invalid `ownerAddress` → `"Invalid address"`.
  - owner account does not exist.
  - invalid `receiverAddress` → `"Invalid receiverAddress"`.
  - receiver account does not exist.
  - receiver is `AccountType.Contract` → `"Do not allow delegate resources to contract addresses"`.
- Delegate amount boundaries:
  - `delegateBalance < 1 TRX` (including `0`/negative) → `"delegateBalance must be greater than or equal to 1 TRX"`.
  - Boundary success at exactly `1 TRX`.
- Resource code validation:
  - unrecognized enum value (e.g. `setResourceValue(999)`) → `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.
- Lock semantics (max-lock feature is currently not exercised):
  - Enable `supportMaxDelegateLockPeriod()` (set `MAX_DELEGATE_LOCK_PERIOD > default`) and add fixtures for:
    - `lockPeriod < 0` and `lockPeriod > max` validation.
    - `validRemainTime(...)` failure when a previous locked delegation has remaining time greater than the new lock period.
    - `lockPeriod == 0` defaulting behavior (`getLockPeriod(...)`).

UnDelegateResourceContract (58) — missing branches / behaviors

- Feature gating:
  - validate-fail when `supportDR() == false`.
  - validate-fail when `supportUnfreezeDelay() == false`.
- Address validation:
  - invalid `ownerAddress` / invalid `receiverAddress`.
  - receiver equals owner (self) → `"receiverAddress must not be the same as ownerAddress"`.
- Amount/resource validation:
  - `unDelegateBalance <= 0` → `"unDelegateBalance must be more than 0 TRX"`.
  - unrecognized resource enum value (e.g. `setResourceValue(999)`) → `"ResourceCode error.valid ResourceCode[BANDWIDTH、Energy]"`.
- Locked delegation availability (currently uncovered):
  - Delegation exists only in the lock record and is not expired (`expireTime >= now`): validate should fail with “insufficient delegated…” because locked balances are only counted when `expireTime < now`.
  - Boundary: `expireTime == now` is still locked (strict `< now`); should fail the same way.
- Execution path gaps:
  - Full undelegate (remove entire delegation) to exercise `DelegatedResourceStore.delete(...)` and
    `DelegatedResourceAccountIndexStore.unDelegateV2(...)` path (current “happy” undelegates only half).
  - Receiver account missing (allowed by validate; “TVM contract suicide” comment): ensure execution succeeds
    and no NPE occurs when `receiverCapsule == null`.

CancelAllUnfreezeV2Contract (59) — missing branches / behaviors

- Feature gating nuance:
  - validate-fail when `unfreezeDelayDays == 0` even if `ALLOW_CANCEL_ALL_UNFREEZE_V2 == 1`
    (because `supportAllowCancelAllUnfreezeV2()` requires both).
- Address/account validation:
  - invalid `ownerAddress` → `"Invalid address"`.
  - owner account does not exist.
- Resource coverage gaps:
  - TRON_POWER unfreeze entries (both expired and unexpired) to pin:
    - `frozenForTronPowerV2` refreeze behavior
    - `totalTronPowerWeight` delta behavior
    - `cancelUnfreezeV2AmountMap["TRON_POWER"]` reporting
- Time boundary:
  - `unfreezeExpireTime == now` treated as expired (`<= now`) and goes to `withdrawExpireAmount` (not refrozen).
- List composition:
  - All entries expired: should behave like “withdraw only” with cancel amounts zero for all resources.
  - Multiple unexpired entries for the same resource: amounts should sum and weight delta should match flooring rules.
  - Non-TRX-multiple unfreeze amounts (e.g. `1 SUN`) to pin rounding in weight math (`/ TRX_PRECISION`).

Notes / Potential fixture-generation pitfalls

- Determinism: this test uses `System.currentTimeMillis()` for dynamic props and tx timestamps; rerunning will
  generate different `request.pb` and DB state. Other generator tests centralize deterministic creation in
  `ConformanceFixtureTestSupport`.
- Block context alignment: `createBlockContext()` here does not update dynamic properties (unlike
  `ConformanceFixtureTestSupport.createBlockContext(dbManager, ...)`). Embedded actuators read “now” from
  `DynamicPropertiesStore`, while the remote request uses `blockCap.getTimeStamp()`. This matters for boundary
  fixtures (`== now`) and lock-expiry checks.
- Seeding delegations: `createDelegation(...)` writes `DelegatedResourceStore` + account balances, but does not
  seed `DelegatedResourceAccountIndexStore`. Any fixture meant to cover index removal/update should seed the index
  store (or create the initial delegation via `DelegateResourceContract` actuator) so post-state deltas are meaningful.
