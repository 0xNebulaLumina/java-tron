# TODO / Fix Plan: `FREEZE_BALANCE_CONTRACT` parity

## Goal

Make Rust `FREEZE_BALANCE_CONTRACT` match Java `FreezeBalanceActuator` semantics for:
- state mutations (self-freeze + delegated-freeze)
- dynamic property side-effects (global weight totals)
- validation error ordering/messages for key malformed cases (when feasible)

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java` (`validate()`, `execute()`, `delegateResource(...)`)
- `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java` (`initializeOldTronPower()`, tron power getters)
- `chainbase/src/main/java/org/tron/core/store/DelegatedResourceAccountIndexStore.java` (optimized index layout + `convert/delegate`)
- `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`allowNewReward()`, `supportAllowDelegateOptimization()`)

---

## Checklist (tactical)

- [ ] Confirm parity scope (what “matching Java” means here)
  - [ ] Decide whether Rust must match Java only for valid txs, or also for malformed tx error strings/order.
  - [ ] Decide whether Rust execution is “authoritative” (must fully validate) vs “best-effort” (Java already validated).

- [ ] Add `oldTronPower` initialization to V1 FreezeBalance (major)
  - [ ] Implement the Java side-effect from `FreezeBalanceActuator.execute()`:
    - [ ] If `ALLOW_NEW_RESOURCE_MODEL` is enabled **and** `account.old_tron_power == 0`, set it to:
      - [ ] `-1` if `getTronPower()` is zero
      - [ ] otherwise `getTronPower()` (snapshot)
    - [ ] Ensure this is done **before** applying the freeze mutation (matches Java ordering).
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_freeze_balance_contract(...)`)
    - [ ] Use `storage_adapter.get_tron_power_in_sun(&owner, false)` as the Java `getTronPower()` equivalent.
  - [ ] Tests
    - [ ] Add a regression test where:
      - [ ] `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=0`, and account has some legacy tron power
      - [ ] After FreezeBalance, `old_tron_power` becomes snapshot value (or `-1` if snapshot is 0).

- [ ] Align “new reward” gating with Java (potentially major)
  - [ ] Replace Rust’s `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` logic with Java’s:
    - [ ] `ALLOW_NEW_REWARD == 1`
  - [ ] Rust touchpoints
    - [ ] Add getter in `rust-backend/crates/execution/src/storage_adapter/engine.rs` for `ALLOW_NEW_REWARD` (8-byte BE long, default 0).
    - [ ] Use it in:
      - [ ] V1 `execute_freeze_balance_contract(...)`
      - [ ] V1 `execute_unfreeze_balance_contract(...)` (same pattern exists there)
  - [ ] Tests
    - [ ] Construct a scenario where `ALLOW_NEW_REWARD=0` but `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` is “active”, and confirm weight deltas follow Java (`amount/TRX_PRECISION`, not `increment`).

- [ ] Implement `ALLOW_DELEGATE_OPTIMIZATION` branch for V1 delegated freeze (major when enabled)
  - [ ] Add dynamic property getter:
    - [ ] `ALLOW_DELEGATE_OPTIMIZATION` (8-byte BE long, default 0)
  - [ ] Implement Java’s optimized index layout + conversion in Rust
    - [ ] Mirror `DelegatedResourceAccountIndexStore.convert(...)`:
      - [ ] If legacy key `address` exists (lists), write new entries with timestamp `i+1` for each list member, then delete legacy key.
    - [ ] Mirror `delegate(from,to,time)`:
      - [ ] Write `0x01 || from21 || to21` → capsule(account=to21, timestamp=time)
      - [ ] Write `0x02 || to21 || from21` → capsule(account=from21, timestamp=time)
    - [ ] During FreezeBalance delegation, when optimization enabled:
      - [ ] `convert(owner)` and `convert(receiver)`
      - [ ] `delegate(owner, receiver, latestBlockHeaderTimestamp)`
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/execution/src/storage_adapter/engine.rs`
      - [ ] Add new methods: `delegate_resource_account_index_v1_optimized(...)`, `convert_delegated_resource_account_index_v1(...)`
      - [ ] Keep existing legacy method for the non-optimized mode.
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs`
      - [ ] Branch index update based on `supportAllowDelegateOptimization()`.
  - [ ] Tests
    - [ ] With `ALLOW_DELEGATE_OPTIMIZATION=1`, assert Rust writes the prefixed keys and deletes legacy key.
    - [ ] Confirm Java’s `getIndex(...)` would reconstruct the same to/from lists from prefix query ordering by timestamp.

- [ ] Preserve Java behavior for unknown `resource` values (edge-case parity)
  - [ ] Change parsing so unknown enum values do not fail early.
    - [ ] Parse `resource` as raw integer; defer validation to match Java switch/default behavior.
    - [ ] Emit Java-equivalent error strings:
      - [ ] new model disabled: `ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]`
      - [ ] new model enabled: `ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]`
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`parse_freeze_balance_params(...)` + validation)
  - [ ] Tests
    - [ ] Unknown resource value should fail with the same message Java would produce.

- [ ] Implement `Any.is(...)`-equivalent validation for FreezeBalance (edge-case parity)
  - [ ] If `transaction.metadata.contract_parameter` is present:
    - [ ] Check `type_url` matches the Java expected type for FreezeBalance (`protocol.FreezeBalanceContract`)
    - [ ] If not, return Java’s “contract type error…” message (match other Rust handlers’ pattern).
  - [ ] Rust touchpoints
    - [ ] `rust-backend/crates/core/src/service/contracts/freeze.rs`
    - [ ] Reuse helper used by other contracts (`any_type_url_matches(...)`) to avoid string drift.

- [ ] Decide what to do about `CommonParameter.checkFrozenTime` (minor)
  - [ ] If test parity requires it, add a config flag in Rust to optionally skip duration bounds.
  - [ ] Otherwise document that Rust always enforces the bound (mainnet behavior).

---

## Verification / rollout checklist

- [ ] `cargo test` under `rust-backend/` with new regression tests
- [ ] Run a small remote-vs-embedded parity slice focused on FreezeBalance (including delegation):
  - [ ] compare account bytes for owner/receiver, `DelegatedResource*` stores, and dynamic property totals
  - [ ] confirm error strings for key malformed fixtures (if included in conformance)
- [ ] Keep `execution.remote.freeze_balance_enabled` gated until parity is confirmed

