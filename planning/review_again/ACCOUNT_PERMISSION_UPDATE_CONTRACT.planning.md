# Review: `ACCOUNT_PERMISSION_UPDATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_account_permission_update_contract()` + helpers
  - `check_account_permission_update_permission()`
  - `parse_account_permission_update_contract()`
  - Source: `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**:
  - `AccountPermissionUpdateActuator` (`validate()` + `execute()`)
    - Source: `actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java`
  - Permission ID assignment via `AccountCapsule.updatePermissions(...)`
    - Source: `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`
  - Fee routing via `DynamicPropertiesStore.supportBlackHoleOptimization()` + `burnTrx(...)`
    - Source: `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`

Goal: determine whether the Rust implementation matches **java-tron actuator semantics** for contract type **46**.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`AccountPermissionUpdateActuator.validate`)

Key checks (error strings shown are the user-visible ones the Rust code tries to match):

1. Multi-sig gate:
   - `ALLOW_MULTI_SIGN` must be `1`, else:
     - `"multi sign is not allowed, need to be opened by the committee"`
2. Contract type:
   - Must be `AccountPermissionUpdateContract`, else:
     - `"contract type error,expected type [AccountPermissionUpdateContract],real type[...]"`
3. Owner address:
   - `DecodeUtil.addressValid(ownerAddress)` (strict prefix + length 21), else:
     - `"invalidate ownerAddress"`
   - Owner account must exist, else:
     - `"ownerAddress account does not exist"`
4. Permission presence and count:
   - Must have `owner` permission (`hasOwner()`), else:
     - `"owner permission is missed"`
   - If account **is witness**:
     - Must have `witness` permission, else:
       - `"witness permission is missed"`
   - If account **is not witness**:
     - Must *not* include `witness` permission, else:
       - `"account isn't witness can't set witness permission"`
   - Must have at least 1 active permission, else:
     - `"active permission is missed"`
   - Active permissions count must be `<= 8`, else:
     - `"active permission is too many"`
5. Permission-level validation (`checkPermission(Permission)`), for owner/witness/actives:
   - keys count:
     - `keysCount > TOTAL_SIGN_NUM` => `"number of keys in permission should not be greater than X"`
     - `keysCount == 0` => `"key's count should be greater than 0"`
     - witness permission must have exactly 1 key => `"Witness permission's key count should be 1"`
   - threshold:
     - must be `> 0` => `"permission's threshold should be greater than 0"`
   - name length:
     - if present, must be `<= 32` => `"permission's name is too long"`
   - parent:
     - `parentId` must be `0` => `"permission's parent should be owner"`
   - key list:
     - addresses must be distinct => `"address should be distinct in permission <type>"`
     - each key address must pass `DecodeUtil.addressValid` => `"key is not a validate address"`
     - each key weight must be `> 0` => `"key's weight should be greater than 0"`
     - `sum(weights) >= threshold` (overflow checked via `addExact`) => `"sum of all key's weight should not be less than threshold in permission <type>"`
   - operations:
     - non-Active permissions must have empty `operations` => `"<type> permission needn't operations"`
     - Active permissions must have `operations.size == 32` => `"operations size must 32"`
     - each enabled bit must be allowed by `AVAILABLE_CONTRACT_TYPE`, else:
       - `"<i> isn't a validate ContractType"`

### 2) Execution (`AccountPermissionUpdateActuator.execute`)

1. Fee: `fee = DynamicPropertiesStore.getUpdateAccountPermissionFee()`
2. Update permissions:
   - `AccountCapsule.updatePermissions(owner, witness, actives)` which *forces IDs*:
     - owner.id = 0
     - witness.id = 1 (only if account is witness)
     - actives[i].id = i + 2
3. Charge fee:
   - `adjustBalance(owner, -fee)` (throws `BalanceInsufficientException` on insufficient balance)
4. Fee destination:
   - If `supportBlackHoleOptimization()`:
     - `burnTrx(fee)` (increments `BURN_TRX_AMOUNT`)
   - Else:
     - credit blackhole account balance by `fee`
5. Receipt: `ret.setStatus(fee, SUCESS)`

Important semantic note: java-tron executes within a transactional / revoking store. If execution throws (e.g., insufficient balance), the net effect is a **revert** (no persisted permission changes, no fee effects).

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### 1) Validation

The Rust handler performs validation inline before applying state:

- Multi-sig gate:
  - uses `storage_adapter.get_allow_multi_sign() -> bool`
  - returns Java-parity error string when `false`
  - **note**: storage adapter treats any non-zero value as `true` and defaults to `true` if missing (differs from Java’s strict `== 1` + “missing key throws”)
- Contract type:
  - checks `Any.type_url` matches `protocol.AccountPermissionUpdateContract` when `transaction.metadata.contract_parameter` is available
  - returns Java-like error string on mismatch
- Contract parsing:
  - `parse_account_permission_update_contract(...)` manually extracts:
    - field 1 `owner_address`
    - field 2 `owner` Permission (presence tracked via `Option`)
    - field 3 `witness` Permission (presence tracked via `Option`)
    - field 4 repeated `actives`
- Owner address:
  - requires 21 bytes with database-derived prefix, else `"invalidate ownerAddress"`
- Account existence:
  - loads `account_proto` from AccountStore; must exist
- Permission presence / witness gating / active count:
  - matches Java’s checks and error strings
- Permission validation:
  - `check_account_permission_update_permission(...)` matches Java rules for:
    - `TOTAL_SIGN_NUM` limit
    - keys non-empty
    - witness key count == 1
    - threshold > 0
    - name length <= 32
    - parent_id == 0
    - distinct key addresses
    - address validity via length/prefix
    - weight > 0 and overflow-safe sum
    - sum(weights) >= threshold
    - operations rules (Active needs 32 bytes; others must be empty)
    - enabled op bits subset of `AVAILABLE_CONTRACT_TYPE`
  - **note**: if `AVAILABLE_CONTRACT_TYPE` is missing/short, Rust falls back to “allow all” instead of Java’s “missing key throws”.

### 2) Execution

- Applies IDs and updates permissions:
  - owner.id = 0
  - witness.id = 1 (only if account is witness)
  - actives[i].id = i + 2
  - clears and replaces `account_proto.active_permission`
- Persists permissions (first write):
  - `storage_adapter.put_account_proto(&owner, &account_proto)`
- Fee:
  - loads `UPDATE_ACCOUNT_PERMISSION_FEE` via `storage_adapter.get_update_account_permission_fee()`
  - checks sufficient balance and subtracts fee from `account_proto.balance`
  - persists updated balance (second write)
- Fee destination:
  - if `support_blackhole_optimization == false`: credits blackhole account by `fee`
  - if `support_blackhole_optimization == true`: **does nothing further** (implicitly “burns” by not crediting any account)
  - **missing behavior**: does **not** increment `BURN_TRX_AMOUNT` (Java uses `burnTrx`)
- Returns:
  - `success = true`, `energy_used = 0`, `bandwidth_used = calculate_bandwidth_usage(transaction)`
  - `tron_transaction_result` includes `fee`

Important semantic note: the Rust backend only gets Java-like atomicity/revert if writes are buffered and committed conditionally. In gRPC mode this depends on `execution.remote.rust_persist_enabled` (see `rust-backend/crates/core/src/service/grpc/mod.rs`).

---

## Does it match java-tron?

### What matches (good parity)

- Validation conditions and most error strings match `AccountPermissionUpdateActuator.validate()`.
- Permission ID assignment matches `AccountCapsule.updatePermissions(...)` (0/1/2+ scheme).
- Permission validation logic matches Java’s `checkPermission(...)`, including:
  - key count rules (including witness key count == 1)
  - threshold/weight sum rules (with overflow detection)
  - address distinctness
  - operations length == 32 for Active and bitmask validation (when `AVAILABLE_CONTRACT_TYPE` is present)
- Fee deduction semantics (when successful) match Java:
  - owner balance decreases by `fee`
  - blackhole credited when blackhole optimization is disabled

### Where it diverges (real mismatches / risk areas)

1) **Blackhole optimization burn accounting is not equivalent**

- Java (`supportBlackHoleOptimization == true`) does:
  - owner balance -= fee
  - `burnTrx(fee)` => increments `BURN_TRX_AMOUNT`
- Rust does:
  - owner balance -= fee
  - does *not* credit blackhole
  - does *not* update `BURN_TRX_AMOUNT`

Impact:

- Dynamic property state diverges (`BURN_TRX_AMOUNT` under-reported).
- Any logic relying on burned amount (supply/accounting/reporting) will differ.

2) **Atomicity / rollback mismatch when writes are not buffered**

- Rust persists permission changes *before* it checks ability to pay the fee.
- If fee payment fails (insufficient balance) and the adapter is in direct-write mode, the DB can end up with:
  - updated permissions
  - unchanged balance
  - no fee routing

Java’s net effect on the same failure is a revert (no persisted changes).

3) **Dynamic properties “missing key” semantics differ**

Java throws `IllegalArgumentException` when dynamic properties are missing (e.g., `ALLOW_MULTI_SIGN`, `TOTAL_SIGN_NUM`, `AVAILABLE_CONTRACT_TYPE`, `UPDATE_ACCOUNT_PERMISSION_FEE`).

Rust storage adapter often uses defaults (and for `AVAILABLE_CONTRACT_TYPE` may allow everything when missing).

Impact:

- On partial/minimal DBs (or early initialization), Rust can accept transactions Java would reject or crash on.
- On real chain DBs, this may be a non-issue if keys are always present and valid.

4) **Boolean dynamic properties interpret “non-zero” as true**

Java uses strict `== 1` checks (e.g., `getAllowMultiSign() != 1` is “not allowed”).

Rust uses `val != 0` in several bool getters.

Impact (edge-case):

- If a DB has unexpected values (e.g., `2`), Rust may allow where Java forbids.

5) **Different behavior for unknown PermissionType values**

Java will surface type errors via the caller checks (`owner permission type is error`, etc.) even for unrecognized enum values.

Rust errors early with `"Invalid permission type"` when enum decoding fails.

Impact:

- Crafted/invalid payloads can yield different error strings.

---

## Bottom line

- The Rust implementation is **close** to Java for “happy-path” permission validation and ID assignment.
- It does **not** fully match Java-side execution semantics because:
  - burn accounting (`BURN_TRX_AMOUNT`) is missing under blackhole optimization
  - failure atomicity can diverge unless buffered commit/revert is enforced
  - dynamic property presence/boolean semantics differ in edge cases

