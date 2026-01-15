Review Target

- `framework/src/test/java/org/tron/core/conformance/FreezeV2FixtureGeneratorTest.java`

Scope

- Fixture generation for V2 freeze/unfreeze:
  - `FreezeBalanceV2Contract` (54)
  - `UnfreezeBalanceV2Contract` (55)
- Baseline flags in this test class (via `initCommonDynamicPropsV2(...)`):
  - `UNFREEZE_DELAY_DAYS = 14` (V2 enabled; `DynamicPropertiesStore.supportUnfreezeDelay() == true`)
  - `ALLOW_NEW_RESOURCE_MODEL = 1` (enables `TRON_POWER` as a valid `ResourceCode`)
  - Total weights initialized to 0 (`totalNetWeight`, `totalEnergyWeight`, `totalTronPowerWeight`)

Current Coverage (as written)

FreezeBalanceV2Contract (54)

- Happy: V2 freeze for `BANDWIDTH`.
- Happy: V2 freeze for `ENERGY`.
- Validate-fail: V2 freeze rejected when `unfreezeDelayDays == 0` (V2 disabled).
- Validate-fail: `frozenBalance > accountBalance`.

UnfreezeBalanceV2Contract (55)

- Happy: V2 unfreeze `BANDWIDTH` (creates a pending `unfrozenV2` entry).
- Validate-fail: unfreeze when there is no `frozenV2` balance for the resource.
- Validate-fail: `unfreezeBalance > frozenAmount`.
- Edge: unfreeze sweeps an expired `unfrozenV2` entry (covers `withdrawExpireAmount` behavior).

Missing Edge Cases (high value for conformance)

Validation paths (source of truth)

- `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceV2Actuator.java`

FreezeBalanceV2Contract (54) — missing validation branches / boundaries

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`): empty / wrong length / wrong prefix bytes.
- Owner account does not exist (`AccountStore.get(ownerAddress) == null`).
- `frozenBalance <= 0`:
  - `0` explicitly (protobuf default-ish inputs).
  - negative value (protobuf int64 allows it via Java builder).
- `frozenBalance < 1 TRX` (fails `TRX_PRECISION` check): e.g. `TRX_PRECISION - 1`.
- Boundary-success fixtures:
  - `frozenBalance == 1 TRX` (minimum allowed).
  - `frozenBalance == accountBalance` (allowed by `<= accountBalance` check).
- Resource coverage gaps:
  - Happy-path `resource = TRON_POWER` (baseline enables it, but no fixture pins behavior).
  - Validate-fail `resource = TRON_POWER` when `ALLOW_NEW_RESOURCE_MODEL = 0` (should reject TRON_POWER only).
  - Invalid / unrecognized enum value (e.g. `setResourceValue(999)`) to lock down default-branch error strings.
- Execution semantics (edge fixtures):
  - Freeze same resource twice (accumulation semantics + total-weight delta behavior).
  - Amounts not multiple of `TRX_PRECISION` (rounding/flooring in weight math): e.g. freeze `N*TRX + 1`.

UnfreezeBalanceV2Contract (55) — missing validation branches / behaviors

- Feature gating: validate-fail when `unfreezeDelayDays == 0` (V2 disabled) for unfreeze as well.
- Invalid `ownerAddress` and “owner account does not exist”.
- Resource coverage gaps:
  - ENERGY: happy-path + validate-fail “no frozen” + validate-fail “too high”.
  - TRON_POWER (when enabled): happy-path + validate-fail “no frozen”.
  - TRON_POWER when `ALLOW_NEW_RESOURCE_MODEL = 0`: validate-fail invalid resource code.
  - Invalid / unrecognized enum value (`setResourceValue(999)`).
- Unfreeze balance boundaries:
  - `unfreezeBalance <= 0` (should fail via `Invalid unfreeze_balance`).
  - `unfreezeBalance == frozenAmount` (unfreeze all) to pin whether the `frozenV2` entry is kept at `0` vs removed.
  - Non-TRX-multiple unfreeze (e.g. unfreeze `1 SUN`) to lock down rounding + weight-delta semantics.
- Pending unfreeze limit:
  - `AccountCapsule.getUnfreezingV2Count(now) >= 32` must validate-fail with “over limit”.
  - Boundary case: `31` pending should still succeed.
- Expired sweep coverage:
  - Multiple expired `unfrozenV2` entries (sweep sum).
  - Mixed expired + unexpired entries (expired swept, unexpired preserved).
  - Expire-time boundary (`expireTime == now` is considered expired in sweep logic).
- Vote side effects (currently entirely uncovered by fixtures):
  - `updateVote(...)` can clear votes (new resource model transition) or rescale votes (legacy model).
  - Conformance fixtures that touch votes must include `votes` DB in `databasesTouched`.

Notes / Potential fixture-generation pitfalls

- No assertions: tests only log `result.isSuccess()` / `validationError`; a mis-specified case can silently
  generate a fixture with unexpected `metadata.expectedStatus` (e.g. “validate_fail” case that actually succeeds).
- Time determinism: `UnfreezeBalanceV2Actuator` uses `DynamicPropertiesStore.getLatestBlockHeaderTimestamp()`;
  craft `unfreezeExpireTime` relative to the block context’s `now` (from `createBlockContext(...)`) to keep
  “expired/not expired/boundary” cases stable.
- When adding vote-related fixtures, include `votes` in `databasesTouched` and seed both `AccountCapsule.votes`
  and/or `VotesStore` as needed so post-state diffs are deterministic.

