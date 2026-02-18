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

- [x] Confirm parity scope (what "matching Java" means here) **COMPLETED**
  - [x] Rust must match Java for valid txs AND for malformed tx error strings/order
  - [x] Rust execution is "authoritative" (must fully validate)

- [x] Add `oldTronPower` initialization to V1 FreezeBalance (major) **COMPLETED**
  - [x] Implement the Java side-effect from `FreezeBalanceActuator.execute()`:
    - [x] If `ALLOW_NEW_RESOURCE_MODEL` is enabled **and** `account.old_tron_power == 0`, set it to:
      - [x] `-1` if `getTronPower()` is zero
      - [x] otherwise `getTronPower()` (snapshot)
    - [x] Ensure this is done **before** applying the freeze mutation (matches Java ordering).
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_freeze_balance_contract(...)`)
    - [x] Use `storage_adapter.get_tron_power_in_sun(&owner, false)` as the Java `getTronPower()` equivalent.
  - [x] Tests
    - [x] Added `test_freeze_initializes_old_tron_power_when_new_resource_model_enabled`:
      - [x] `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=0`, account has 5 TRX legacy frozen
      - [x] After FreezeBalance, `old_tron_power` becomes 5_000_000 (snapshot value)
    - [x] Added `test_freeze_initializes_old_tron_power_to_minus_one_when_legacy_power_is_zero`:
      - [x] `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=0`, no legacy frozen
      - [x] After FreezeBalance, `old_tron_power` becomes `-1`

- [x] Align "new reward" gating with Java (potentially major) **COMPLETED**
  - [x] Replace Rust's `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` logic with Java's:
    - [x] `ALLOW_NEW_REWARD == 1`
  - [x] Rust touchpoints
    - [x] Add getter in `rust-backend/crates/execution/src/storage_adapter/engine.rs` for `ALLOW_NEW_REWARD` (8-byte BE long, default 0).
      - Added `get_allow_new_reward()` and `allow_new_reward()` methods
    - [x] Use it in:
      - [x] V1 `execute_freeze_balance_contract(...)` - uses `allow_new_reward()` instead of cycle-based check
      - [x] V1 `execute_unfreeze_balance_contract(...)` - uses `allow_new_reward()` instead of cycle-based check
  - [ ] Tests
    - [ ] Construct a scenario where `ALLOW_NEW_REWARD=0` but `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` is "active", and confirm weight deltas follow Java (`amount/TRX_PRECISION`, not `increment`).

- [x] Implement `ALLOW_DELEGATE_OPTIMIZATION` branch for V1 delegated freeze (major when enabled) **COMPLETED**
  - [x] Add dynamic property getter:
    - [x] `ALLOW_DELEGATE_OPTIMIZATION` (8-byte BE long, default 0)
    - Added `get_allow_delegate_optimization()` and `support_allow_delegate_optimization()` methods
  - [x] Implement Java's optimized index layout + conversion in Rust
    - [x] Mirror `DelegatedResourceAccountIndexStore.convert(...)`:
      - [x] If legacy key `address` exists (lists), write new entries with timestamp `i+1` for each list member, then delete legacy key.
      - Implemented `convert_delegated_resource_account_index_v1()` method
    - [x] Mirror `delegate(from,to,time)`:
      - [x] Write `0x01 || from21 || to21` → capsule(account=to21, timestamp=time)
      - [x] Write `0x02 || to21 || from21` → capsule(account=from21, timestamp=time)
      - Implemented `delegate_v1_optimized()` method
    - [x] During FreezeBalance delegation, when optimization enabled:
      - [x] `convert(owner)` and `convert(receiver)`
      - [x] `delegate(owner, receiver, latestBlockHeaderTimestamp)`
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/execution/src/storage_adapter/engine.rs`
      - [x] Add new methods: `delegate_v1_optimized(...)`, `convert_delegated_resource_account_index_v1(...)`
      - [x] Added `undelegate_v1_optimized()` for V1 unfreeze with optimization enabled
      - [x] Keep existing legacy method for the non-optimized mode.
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs`
      - [x] Branch index update based on `supportAllowDelegateOptimization()`.
      - Updated both Bandwidth and Energy delegation paths in `execute_freeze_balance_contract`
      - Updated unfreeze delegation cleanup in `execute_unfreeze_balance_contract`
  - [ ] Tests
    - [ ] With `ALLOW_DELEGATE_OPTIMIZATION=1`, assert Rust writes the prefixed keys and deletes legacy key.
    - [ ] Confirm Java's `getIndex(...)` would reconstruct the same to/from lists from prefix query ordering by timestamp.

- [x] Preserve Java behavior for unknown `resource` values (edge-case parity) **COMPLETED**
  - [x] Change parsing so unknown enum values do not fail early.
    - [x] Added `FreezeResource::Unknown` enum variant
    - [x] Parse `resource` as raw integer; defer validation to match Java switch/default behavior
    - [x] Emit Java-equivalent error strings:
      - [x] new model disabled: `ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]`
      - [x] new model enabled: `ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]`
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs` (`parse_freeze_balance_params(...)` + validation)
    - [x] Added `resource_raw` field to `FreezeParams` and `UnfreezeParams` for tracking raw value
    - [x] Updated all `match params.resource` blocks to handle `FreezeResource::Unknown`
  - [x] Tests
    - [x] Added `test_freeze_unknown_resource_returns_java_error_message`:
      - [x] Unknown resource code (99) fails with Java-parity "ResourceCode error" message

- [x] Implement `Any.is(...)`-equivalent validation for FreezeBalance (edge-case parity) **COMPLETED**
  - [x] If `transaction.metadata.contract_parameter` is present:
    - [x] Check `type_url` matches the Java expected type for each contract type
    - [x] If not, return Java's "contract type error..." message
  - [x] Rust touchpoints
    - [x] `rust-backend/crates/core/src/service/contracts/freeze.rs`
    - [x] Added validation to all four functions:
      - [x] `execute_freeze_balance_contract` - checks for "FreezeBalanceContract"
      - [x] `execute_unfreeze_balance_contract` - checks for "UnfreezeBalanceContract"
      - [x] `execute_freeze_balance_v2_contract` - checks for "FreezeBalanceV2Contract"
      - [x] `execute_unfreeze_balance_v2_contract` - checks for "UnfreezeBalanceV2Contract"

- [x] Decide what to do about `CommonParameter.checkFrozenTime` (minor) **DOCUMENTED**
  - [x] Rust always enforces the duration bounds (mainnet behavior)
  - [x] Java's `checkFrozenTime` flag is for test environments; Rust follows mainnet semantics
  - Note: If test parity requires skipping duration checks, a config flag can be added later

---

## Verification / rollout checklist

- [x] `cargo test` under `rust-backend/` with new regression tests
  - All 9 freeze_balance tests pass
  - 212 total tests pass (3 pre-existing VoteWitness test failures unrelated to freeze changes)
- [ ] Run a small remote-vs-embedded parity slice focused on FreezeBalance (including delegation):
  - [ ] compare account bytes for owner/receiver, `DelegatedResource*` stores, and dynamic property totals
  - [ ] confirm error strings for key malformed fixtures (if included in conformance)
- [ ] Keep `execution.remote.freeze_balance_enabled` gated until parity is confirmed

---

## Implementation Summary

### Changes Made (2025-02-15)

1. **oldTronPower Initialization** (`freeze.rs`)
   - Added initialization of `old_tron_power` field before applying freeze mutation
   - Uses `storage_adapter.get_tron_power_in_sun(&owner, false)` to get legacy tron power
   - Sets to `-1` if tron power is zero, otherwise snapshot value

2. **ALLOW_NEW_REWARD Gating** (`engine.rs`, `freeze.rs`)
   - Added `get_allow_new_reward()` and `allow_new_reward()` methods to storage adapter
   - Replaced cycle-based `CURRENT_CYCLE_NUMBER >= NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE` check
   - Now uses `ALLOW_NEW_REWARD == 1` (Java parity)
   - Applied to both V1 freeze and V1 unfreeze contracts

3. **ALLOW_DELEGATE_OPTIMIZATION Support** (`engine.rs`, `freeze.rs`)
   - Added `get_allow_delegate_optimization()` and `support_allow_delegate_optimization()` methods
   - Implemented optimized delegation index methods:
     - `convert_delegated_resource_account_index_v1()` - migrates legacy lists to prefix keys
     - `delegate_v1_optimized()` - writes `0x01||from||to` and `0x02||to||from` keys
     - `undelegate_v1_optimized()` - deletes optimized prefix keys
   - Updated freeze/unfreeze delegation paths to use optimized layout when enabled

4. **Test Fixes** (`freeze_balance.rs`)
   - Fixed tests to properly set up Account proto with frozen fields
   - Added required dynamic properties (`UNFREEZE_DELAY_DAYS`, `latest_block_header_timestamp`)
   - Fixed FreezeV2/UnfreezeV2 tests to include owner_address in protobuf data
   - Fixed `FreezeV2.r#type` field type (i32 not Option<i32>)

### Changes Made (2025-02-18)

5. **Unknown Resource Value Handling** (`freeze.rs`)
   - Added `FreezeResource::Unknown` enum variant for unknown resource codes
   - Added `resource_raw: i64` field to `FreezeParams` and `UnfreezeParams`
   - Changed parsing to store raw value and defer validation (matches Java switch/default behavior)
   - Added Java-parity error messages for unknown resource codes:
     - New model disabled: "ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY]"
     - New model enabled: "ResourceCode error, valid ResourceCode[BANDWIDTH、ENERGY、TRON_POWER]"
   - Updated all `match params.resource` blocks to handle `FreezeResource::Unknown`

6. **Any.is() Validation** (`freeze.rs`)
   - Added type_url validation for protobuf Any wrapper (Java parity)
   - Checks `transaction.metadata.contract_parameter.type_url` if present
   - Returns Java-style "contract type error,expected type [X],real type[Y]" on mismatch
   - Added to all four freeze contract functions:
     - `execute_freeze_balance_contract` - checks "FreezeBalanceContract"
     - `execute_unfreeze_balance_contract` - checks "UnfreezeBalanceContract"
     - `execute_freeze_balance_v2_contract` - checks "FreezeBalanceV2Contract"
     - `execute_unfreeze_balance_v2_contract` - checks "UnfreezeBalanceV2Contract"

