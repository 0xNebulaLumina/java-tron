Review Target

- `framework/src/test/java/org/tron/core/conformance/FreezeV1FixtureGeneratorTest.java`

Scope

- Fixture generation for V1 freeze/unfreeze:
  - `FreezeBalanceContract` (11)
  - `UnfreezeBalanceContract` (12)
- Baseline flags in this test class:
  - `UNFREEZE_DELAY_DAYS = 0` (V2 closed; V1 freeze enabled)
  - `ALLOW_NEW_RESOURCE_MODEL = 0` (no `TRON_POWER`)
  - Delegation is effectively disabled (`DynamicPropertiesStore.supportDR() == false`)

Current Coverage (as written)

FreezeBalanceContract (11)

- Happy: V1 freeze for `BANDWIDTH`.
- Happy: V1 freeze for `ENERGY`.
- Validate-fail: V1 freeze rejected when `unfreezeDelayDays > 0` (V2 enabled).
- Validate-fail: `frozenBalance < 1 TRX`.
- Validate-fail: `frozenBalance > accountBalance`.

UnfreezeBalanceContract (12)

- Happy: V1 unfreeze `BANDWIDTH` when a frozen entry is expired.
- Validate-fail: V1 unfreeze `BANDWIDTH` when frozen entry is not expired.
- Validate-fail: V1 unfreeze `BANDWIDTH` when no frozen balance exists.

Missing Edge Cases (high value for conformance)

Validation paths (source of truth)

- `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java`

FreezeBalanceContract (11) — missing validation branches / boundaries

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`): empty / wrong length / wrong prefix.
- Owner account does not exist (`AccountStore.get(ownerAddress) == null`).
- `frozenBalance <= 0` (`0` explicitly; negative if protobuf/builder allows).
- Boundary-success fixtures:
  - `frozenBalance == 1 TRX` (minimum allowed).
  - `frozenBalance == accountBalance` (allowed by `<= accountBalance` check).
- Frozen duration validation (enabled when `CommonParameter.checkFrozenTime == 1`):
  - `frozenDuration < minFrozenTime`.
  - `frozenDuration > maxFrozenTime`.
  - Note: `DynamicPropertiesStore` defaults both min/max frozen time to `3`, so “too long” and “too short”
    can both be exercised (e.g., `2` and `4`) but the error message includes both bounds.
- Pre-state guard: `frozenCount` must be `0` or `1` (`"frozenCount must be 0 or 1"`). This requires a
  crafted account with 2 `frozen` entries (illegal state, but explicitly validated).
- Invalid `resource` handling:
  - `TRON_POWER` while `allowNewResourceModel=0` should fail with
    `"ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"`.
  - Unrecognized enum value should fail with the corresponding “valid ResourceCode[...]” message.
- Receiver/delegation handling (not exercised today):
  - If delegation is OFF but `receiverAddress` is set, Java-tron ignores it (validates/executed as self-freeze);
    a dedicated `edge` fixture can lock this behavior for cross-impl conformance.
  - If delegation is ON (`allowDelegateResource=1`), add fixtures for:
    - `receiverAddress == ownerAddress`
    - invalid receiver address bytes
    - receiver account missing
    - (when `allowTvmConstantinople=1`) receiver account type is Contract
- Multi-freeze execution semantics:
  - Freeze `BANDWIDTH` twice to pin down balance accumulation + expireTime update semantics.

UnfreezeBalanceContract (12) — missing validation branches / behaviors

- Invalid `ownerAddress` and “owner account does not exist”.
- Resource coverage gaps:
  - ENERGY: happy-path expired unfreeze; validate-fail not-expired; validate-fail no frozen.
  - `TRON_POWER` while `allowNewResourceModel=0` should validate-fail with invalid resource code.
  - (Optional) `TRON_POWER` unfreeze fixtures when `allowNewResourceModel=1` (relevant if TRON_POWER was
    frozen via V1 before V2 opened).
- Expiration boundary:
  - For BANDWIDTH, `expireTime == now` should be unfreezable (validate uses `<= now`), so add an `edge`
    fixture to pin the boundary.
- BANDWIDTH multiple frozen entries:
  - Partial unfreeze: one expired + one not expired (execute should unfreeze only the expired amount).
  - Multiple expired entries: execute should unfreeze the sum of all expired entries.
- Receiver/delegation handling:
  - If delegation is OFF but `receiverAddress` is set, Java-tron ignores it (self-unfreeze); add a fixture.
  - If delegation is ON (`allowDelegateResource=1`), add delegated unfreeze fixtures:
    - delegated resource entry missing (`"delegated Resource does not exist"`)
    - no delegated frozen balance (`"no delegatedFrozenBalance(BANDWIDTH)"` / `"no delegateFrozenBalance(Energy)"`)
    - not time (`"It's not time to unfreeze."`)
- V2-open compatibility:
  - `UnfreezeBalanceActuator` does not check `supportUnfreezeDelay()`, so V1 unfreeze should still work even
    when V2 is open; a fixture with `unfreezeDelayDays > 0` + a legacy V1 frozen entry is useful to lock in
    cross-implementation behavior.

Notes / Potential fixture-generation pitfalls

- Time determinism: `createBlockContext(dbManager, ...)` advances and persists latest block time/height/hash;
  when crafting frozen `expireTime`, set it relative to `DynamicPropertiesStore.getLatestBlockHeaderTimestamp()`
  so “expired/not expired/boundary” cases are stable.
- Delegation fixtures should include `DelegatedResource` and `DelegatedResourceAccountIndex` DBs; if enabling
  `CHANGE_DELEGATION` (reward delegation), include `delegation` as well.
- Unfreeze always writes/clears `votes` when `allowNewResourceModel=0`; keep `votes` in metadata even if you
  don’t seed votes, otherwise post-state diffs can be surprising to fixture consumers.

