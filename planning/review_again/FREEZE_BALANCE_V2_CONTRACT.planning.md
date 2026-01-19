# FREEZE_BALANCE_V2_CONTRACT parity review (Rust vs Java)

## Scope
Review whether the Rust backend implementation of `FREEZE_BALANCE_V2_CONTRACT` matches the canonical Java behavior (java-tron).

### Files reviewed
- Java actuator (canonical behavior): `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java`
- Java V2 freeze semantics used by CSV/domain reporting:
  - `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` (pre-state snapshot capture)
  - `framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java` (freeze delta conversion)
- Rust execution: `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_freeze_balance_v2_contract`)
- Rust tests that encode intended semantics: `rust-backend/crates/core/src/service/tests/contracts.rs`

## Java-side (canonical) behavior summary

### Validation (`FreezeBalanceV2Actuator#validate`)
1. Feature gate: `dynamicStore.supportUnfreezeDelay()` must be true, else:
   - `"Not support FreezeV2 transaction, need to be opened by the committee"`
2. `owner_address` must pass `DecodeUtil.addressValid` (21 bytes, prefix 0x41), else `"Invalid address"`.
3. Owner account must exist, else `"Account[<readable>] not exists"`.
4. `frozen_balance`:
   - `> 0`
   - `>= TRX_PRECISION` (1 TRX)
   - `<= accountBalance`
5. `resource` must be:
   - `BANDWIDTH` or `ENERGY`, always
   - `TRON_POWER` only if `dynamicStore.supportAllowNewResourceModel()`

### Execute (`FreezeBalanceV2Actuator#execute`)
1. If `supportAllowNewResourceModel()` and `oldTronPowerIsNotInitialized()` (`old_tron_power == 0`), then:
   - `initializeOldTronPower()` sets `old_tron_power = getTronPower()` or `-1` if computed is 0.
2. For the chosen resource, update frozen V2 amount (aggregate by resource type):
   - BANDWIDTH: `addFrozenBalanceForBandwidthV2(frozenBalance)`
   - ENERGY: `addFrozenBalanceForEnergyV2(frozenBalance)`
   - TRON_POWER: `addFrozenForTronPowerV2(frozenBalance)`
3. Update global weights (delta-based, integer division by `TRX_PRECISION`):
   - For BANDWIDTH/ENERGY: weight uses `getFrozenV2BalanceWithDelegated(resource) / TRX_PRECISION`
   - For TRON_POWER: weight uses `getTronPowerFrozenV2Balance() / TRX_PRECISION`
4. Deduct owner balance by `frozen_balance`.
5. Domain journaling (when enabled):
   - Records freeze change with **no expiration** for V2 (`oldExpireTime=0`, `newExpireTime=0`).

## Rust-side behavior summary (`execute_freeze_balance_v2_contract`)

### What matches Java well
- Same feature gate (`support_unfreeze_delay`) with same error string.
- Same frozen_balance checks and error messages.
- Same resource validation rules for `TRON_POWER` gated by `support_allow_new_resource_model`.
- Same `old_tron_power` initialization semantics (`0 -> getTronPower(), 0 => -1`).
- Updates `Account.frozen_v2` by aggregating the matching resource entry (or creating one).
- Updates global weights using the same formula: `(frozen_v2 + delegated_frozen_v2) / TRX_PRECISION` (and TP is just frozen_v2 TP sum).
- Deducts `Account.balance` by `frozen_balance`.

### Key non-Java behavior currently present in Rust
1. **Custom “freeze ledger” persistence**
   - Rust writes into a custom DB (`freeze-records`) via `add_freeze_amount(...)`.
   - Java-tron does not have this database; consensus state is the account proto (`frozenV2`, balance, dynamic props).

2. **Expiration timestamp is synthesized for V2**
   - Rust sets `expiration_timestamp = block_timestamp + 3 days` for V2 freezes.
   - It then emits that expiration in `FreezeLedgerChange.expiration_ms` (when `emit_freeze_ledger_changes` is enabled).

## Parity assessment

### On-chain / canonical state (Account + DynamicProperties)
For `frozenV2` amounts, owner balance, and the dynamic total weights, Rust is very close to Java and appears logically aligned.

### CSV / domain reporting parity (freeze deltas)
Rust does **not** match Java semantics today for V2 expiration reporting:
- Java explicitly treats V2 freeze as having **no expiration** (0).
  - `FreezeBalanceV2Actuator` records `oldExpireTime=0` and `newExpireTime=0`.
  - `RuntimeSpiImpl.capturePreStateSnapshot(...)` hard-codes `oldExpireTimeMs = 0L` for v2 and comments “V2 has no expiration”.
- Rust emits a **non-zero** `expiration_ms` for V2.

Impact:
- Remote-mode CSV/domain freeze delta will show `newExpireTimeMs != 0` for V2 freezes, diverging from embedded mode and Java’s intended semantics.
- This is visible wherever `DomainCanonicalizer.convertFreezeChanges(...)` is used.

### Additional red flags (implementation/test consistency)
- `parse_freeze_balance_v2_params(...)` comment says owner_address is taken from `transaction.from`, but the implementation currently *requires/parses* it from `transaction.data`.
- Rust unit tests in `rust-backend/crates/core/src/service/tests/contracts.rs` are currently inconsistent with the executor’s address validation expectations (e.g., `test_freeze_balance_v2_emits_with_v2_flag` fails because it omits `owner_address` and `from_raw`-style validation data).
  - This does not prove a Java parity bug by itself, but it indicates the Rust-side “wire format” expectations for system-contract `data`/`from_raw` aren’t clearly settled.

## Conclusion
**Does Rust match Java-side logic?**
- **Mostly yes** for the *canonical state transition* of `FREEZE_BALANCE_V2_CONTRACT` (balance deduction, `frozenV2` aggregation, total weight delta updates, oldTronPower initialization).
- **No** for the *V2 expiration semantics* that Java explicitly defines as “no expiration”: Rust currently synthesizes and emits a 3-day expiration, which will break CSV/domain parity between remote execution and embedded Java.

## Recommended fixes (high level)
See `planning/review_again/FREEZE_BALANCE_V2_CONTRACT.todo.md`.

