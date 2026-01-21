# Review: `CANCEL_ALL_UNFREEZE_V2_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_cancel_all_unfreeze_v2_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `CancelAllUnfreezeV2Actuator` in `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`
- **Receipt builder (Rust)**: `TransactionResultBuilder` in `rust-backend/crates/core/src/service/contracts/proto.rs` (fields `withdraw_expire_amount` + `cancel_unfreezeV2_amount`)

Goal: determine whether Rust matches **java-tron’s actuator semantics** (validation + state transitions + receipt encoding).

---

## Java-side reference behavior (what “correct” means)

Source: `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`

### 1) Validation (`validate`)

Key checks:

1. `any.is(CancelAllUnfreezeV2Contract.class)` (type check)
2. `dynamicStore.supportAllowCancelAllUnfreezeV2()` gate:
   - `ALLOW_CANCEL_ALL_UNFREEZE_V2 == 1`
   - `UNFREEZE_DELAY_DAYS > 0`
3. `DecodeUtil.addressValid(ownerAddress)` (21 bytes + correct prefix)
4. Owner account exists
5. Owner `unfrozenV2List` is **not empty** (`"No unfreezeV2 list to cancel"`)

### 2) Execution (`execute`)

High-level behavior:

- `fee = 0`
- `now = latestBlockHeaderTimestamp`
- Iterate `Account.unfrozenV2` entries:
  - If `unfreeze_expire_time > now` (unexpired):
    - **Re-freeze** the amount back into `Account.frozenV2` for the entry’s resource type
    - Update **global total weights** (`TOTAL_NET_WEIGHT`, `TOTAL_ENERGY_WEIGHT`, `TOTAL_TRON_POWER_WEIGHT`) by:
      - `delta = floor((frozenV2 + delegatedFrozenV2) / TRX_PRECISION)` after
      - minus the same value before
    - Accumulate `cancel_unfreezeV2_amount[resource] += unfreeze_amount`
  - Else (expired, i.e. `<= now`):
    - Accumulate `withdraw_expire_amount += unfreeze_amount`
- Clear `Account.unfrozenV2`
- If `withdraw_expire_amount > 0`, add to `Account.balance`
- Persist account
- Receipt fields:
  - `withdraw_expire_amount` (field 27)
  - `cancel_unfreezeV2_amount` map (field 28) with keys:
    - `"BANDWIDTH"`, `"ENERGY"`, `"TRON_POWER"`
    - **All three keys are present**, even when value is `0`

---

## Rust backend behavior (current)

Source: `rust-backend/crates/core/src/service/mod.rs` (`execute_cancel_all_unfreeze_v2_contract`)

### Validation & gating

- Type URL check when `transaction.metadata.contract_parameter` is present:
  - expects `protocol.CancelAllUnfreezeV2Contract`
- Gate check: `storage_adapter.support_allow_cancel_all_unfreeze_v2()` (matches Java’s `ALLOW_CANCEL_ALL_UNFREEZE_V2 == 1 && UNFREEZE_DELAY_DAYS > 0`)
- Address validation uses `transaction.metadata.from_raw`:
  - requires 21 bytes + `storage_adapter.address_prefix()`
- Owner account existence check
- Rejects empty `account_proto.unfrozen_v2` with `"No unfreezeV2 list to cancel"`

### State transitions

The loop matches Java’s expired vs unexpired split:

- Expired (`unfreeze_expire_time <= now`): sums `withdraw_expire_amount`
- Unexpired: re-freezes into `frozen_v2` by type and updates:
  - `net_weight_delta`, `energy_weight_delta` using `frozen_v2 + delegated_frozen_v2` / `TRX_PRECISION`
  - `tp_weight_delta` using tron-power `frozen_v2` / `TRX_PRECISION`
- Clears `unfrozen_v2`
- Adds `withdraw_expire_amount` to `balance` only when `> 0`
- Applies total weight deltas to dynamic properties
- Persists updated account

### Receipt encoding (Rust)

Receipt bytes are built via:

- `TransactionResultBuilder::with_withdraw_expire_amount(withdraw_expire_amount)`
- `TransactionResultBuilder::with_cancel_unfreeze_v2_amounts(cancel_bandwidth, cancel_energy, cancel_tron_power)`

Current `with_cancel_unfreeze_v2_amounts` behavior:

- Only includes keys where the amount is `> 0`
- Omits the entire map when all cancel amounts are `0`

---

## Parity assessment

### ✅ Core actuator semantics: state changes match

For “normal” inputs (where `transaction.metadata.from_raw` matches the contract’s `owner_address`), Rust matches Java on:

- Feature gating (`ALLOW_CANCEL_ALL_UNFREEZE_V2 == 1 && UNFREEZE_DELAY_DAYS > 0`)
- Expired vs unexpired split (`expire_time <= now` treated as expired/withdrawn)
- Re-freeze behavior per resource type (BANDWIDTH / ENERGY / TRON_POWER)
- Global weight deltas using `floor((frozenV2 + delegatedFrozenV2) / TRX_PRECISION)` (bandwidth/energy) and `floor(tronPowerFrozenV2 / TRX_PRECISION)` (tron power)
- Clearing `unfrozenV2` list and balance update for expired amounts

### ❌ Receipt parity: does **not** match Java

This is observable directly in the conformance fixture oracles:

- `conformance/fixtures/cancel_all_unfreeze_v2_contract/happy_path/expected/result.pb` decodes as:
  - `cancel_unfreezeV2_amount` includes **all three keys**, with `ENERGY=0`, `TRON_POWER=0`, `BANDWIDTH=...`
  - `withdraw_expire_amount` is **absent** when it is 0
- `conformance/fixtures/cancel_all_unfreeze_v2_contract/edge_all_entries_expired_withdraw_only/expected/result.pb` decodes as:
  - `withdraw_expire_amount` is present (non-zero)
  - `cancel_unfreezeV2_amount` includes **all three keys**, each explicitly `0`

By contrast, Rust currently:

1. **Omits** map entries for `ENERGY`/`TRON_POWER` when their cancel amounts are 0.
2. Likely **encodes** `withdraw_expire_amount` even when it is 0 (because the builder is called unconditionally).

If/when conformance starts comparing `expected/result.pb`, Rust will fail on this contract even if state transitions are correct.

### ⚠️ Source-of-truth mismatch: owner address parsing

Java uses `owner_address` from the decoded `CancelAllUnfreezeV2Contract`.

Rust uses `transaction.from` / `transaction.metadata.from_raw` and does **not** decode `owner_address` from `contract_parameter.value`.

This works when upstream guarantees equality, but it is not strictly the same logic and can diverge for:

- malformed / mismatched fixtures
- any future call path where `from_raw` is absent but `contract_parameter.value` is present

---

## Conclusion

- **State logic parity**: ✅ looks aligned with java-tron’s `CancelAllUnfreezeV2Actuator`.
- **Receipt parity**: ❌ not aligned (map key presence + likely field-27 presence when 0).
- **Robustness vs Java parsing**: ⚠️ Rust should ideally parse `owner_address` from the contract bytes for true equivalence.

