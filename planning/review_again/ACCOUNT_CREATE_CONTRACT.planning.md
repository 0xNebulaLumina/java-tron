# Review: `ACCOUNT_CREATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_account_create_contract()` + `parse_account_create_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `CreateAccountActuator` and `AccountCapsule(AccountCreateContract, ...)`

It focuses on whether the Rust implementation matches **java-tron’s contract/actuator semantics**. It also calls out **surrounding Java-side behavior** that can affect end-to-end parity when remote execution is expected to cover more than the actuator.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`CreateAccountActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`

- Rejects wrong contract type: `any.is(AccountCreateContract.class)`
- Validates both addresses with `DecodeUtil.addressValid(...)`
  - Requires **21 bytes**
  - Requires **prefix byte == `DecodeUtil.addressPreFixByte`** (mainnet `0x41`, testnet `0xa0`)
- Requires owner account exists: `accountStore.get(ownerAddress) != null`
- Requires owner balance >= `getCreateNewAccountFeeInSystemContract()`
- Requires target account does **not** exist: `!accountStore.has(accountAddress)`

### 2) Execution (`CreateAccountActuator.execute`)

Sources:

- `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
- `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`

Steps:

1. Fee `fee = DynamicPropertiesStore.getCreateNewAccountFeeInSystemContract()`
2. Build new account capsule:
   - `create_time = DynamicPropertiesStore.getLatestBlockHeaderTimestamp()`
   - If `ALLOW_MULTI_SIGN == 1`, sets default permissions:
     - owner permission id=0, name `owner`, threshold=1, key=(newAccountAddress, weight=1)
     - active permission id=2, name `active`, threshold=1, ops=`ACTIVE_DEFAULT_OPERATIONS`, same key
3. Persist new account into `AccountStore` at `account_address`
4. Deduct `fee` from owner balance (no-op when `fee == 0`)
5. Fee destination:
   - If `supportBlackHoleOptimization() == true`: increments `BURN_TRX_AMOUNT`
   - Else: credits the blackhole account balance by `fee`
6. Sets receipt status: `ret.setStatus(fee, SUCESS)`

### 3) Surrounding Java behavior (important for end-to-end parity)

`AccountCreateContract` is also treated by `BandwidthProcessor` as “creates a new account”.

Source: `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`

- `contractCreateNewAccount(contract)` returns `true` for `AccountCreateContract`
- Resource consumption uses the **create-account path**:
  - tries bandwidth with **`netCost = bytes * CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`**
  - if insufficient bandwidth: charges **`CREATE_ACCOUNT_FEE`** instead and updates `TOTAL_CREATE_ACCOUNT_COST`

This behavior is *not part of `CreateAccountActuator`*, but it matters if the Rust remote execution result is expected to fully reproduce embedded node behavior.

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### Validation & parsing

- If `transaction.metadata.contract_parameter` is present:
  - checks Any type url matches `protocol.AccountCreateContract`
  - returns Java-matching error string on mismatch
- Parses `AccountCreateContract` bytes manually:
  - reads field 1: `owner_address` (expects 21 bytes w/ prefix)
  - reads field 2: `account_address` (expects 21 bytes w/ prefix)
  - skips field 3 `type` (explicitly ignored)
- Validates:
  - owner account exists (`storage_adapter.get_account(owner)` must be `Some`)
  - target account does not exist
  - fee fetched from `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
  - owner balance >= fee; otherwise returns exact Java error string

### State changes / persistence

- Owner balance deduction (only if `fee > 0`)
- Creates target account:
  - writes a full `protocol::Account` proto with:
    - address (21-byte TRON key)
    - create_time = latest block header timestamp
    - optional default permissions if `ALLOW_MULTI_SIGN` is enabled
  - (does not set `Account.type/typeValue` explicitly; relies on proto defaults)
- Fee destination:
  - if `support_black_hole_optimization()`: increments `BURN_TRX_AMOUNT` (`burn_trx`)
  - else: loads blackhole account, increments its balance, persists it
- Returns:
  - `energy_used = 0`
  - `bandwidth_used = calculate_bandwidth_usage(transaction)` (currently a simplified byte estimate)
  - account-level `state_changes` for owner/target/(optional blackhole), sorted by address
  - if AEXT mode is `tracked`: runs `ResourceTracker::track_bandwidth` and persists owner AEXT

---

## Does it match java-tron?

### What matches (good parity vs `CreateAccountActuator`)

- **Fee source**: uses `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- **Owner existence check** + **target non-existence check**
- **Insufficient fee error string**: `"Validate CreateAccountActuator error, insufficient fee."`
- **Fee destination logic**: burn vs credit blackhole keyed off `ALLOW_BLACKHOLE_OPTIMIZATION`
- **Default permission construction** when multi-sign enabled matches java-tron:
  - ids (`0`, `2`), names (`owner`, `active`), threshold=1, parent_id=0, key weight=1
  - active ops sourced from `ACTIVE_DEFAULT_OPERATIONS`
- **create_time** is taken from latest block header timestamp (Java uses the same source)
- **fee==0 behavior**: owner/blackhole aren’t modified when fee is zero (Java’s `adjustBalance` returns early too)

### Where it diverges (real mismatches / risk areas)

1) **Address prefix validation is more permissive than Java**

- Rust parser accepts address prefix `0x41` *or* `0xa0` unconditionally.
- Java validates against **exactly one configured prefix** (`DecodeUtil.addressPreFixByte`).

Impact:

- Rust may accept a wrong-prefix address that Java rejects (or fail later with “account not exists” instead of “Invalid ownerAddress”).
- For testnet/mainnet toggles, Rust should ideally validate against `storage_adapter.address_prefix()` (or equivalent), not a fixed allowlist.

2) **Contract `type` field is ignored**

- Java persists `Account.type`/`typeValue` based on `contract.getType()`/`getTypeValue()`.
- Rust explicitly skips field 3 and always stores the proto default (effectively `Normal`).

Impact:

- For non-`Normal` values (or unknown enum values), persisted account bytes will diverge.

3) **Dynamic property “missing key” behavior differs**

- Java throws if critical dynamic properties are missing from DB (e.g., `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`, `ALLOW_MULTI_SIGN`).
- Rust generally falls back to defaults when keys are absent.
- Note: java-tron’s `DynamicPropertiesStore` constructor eagerly seeds many missing keys with defaults at startup (by catching `IllegalArgumentException` and saving), so “missing key throws” typically only shows up with partial/corrupted DBs or fixtures that bypass initialization.

Impact:

- In minimal/partial fixtures, Rust may “succeed” where Java would error out, masking DB initialization issues.
- Rust’s default fallbacks approximate java-tron’s startup defaults, but do not persist the missing keys.

4) **Resource/bandwidth semantics do not match Java’s create-account bandwidth path**

If remote execution is expected to cover the BandwidthProcessor semantics (or to update AEXT consistently with embedded):

- Java uses `netCost = bytes * CREATE_NEW_ACCOUNT_BANDWIDTH_RATE` for create-account transactions.
- Java can fall back to charging `CREATE_ACCOUNT_FEE` when bandwidth is insufficient and updates totals.
- Rust currently:
  - uses a simplified `calculate_bandwidth_usage()`
  - `ResourceTracker::track_bandwidth()` is explicitly “Phase 1 simplified” and does not model the create-account path

Impact:

- `bandwidth_used` and any tracked AEXT changes can diverge from embedded semantics for this contract.

5) **Receipt passthrough**

- Java actuator sets `TransactionResultCapsule` status/fee (`ret.setStatus(fee, SUCESS)`).
- Rust does not populate `tron_transaction_result` for this contract.

Impact:

- If Java-side receipt building relies on `tron_transaction_result` in remote mode, this may create receipt parity gaps.

---

## Bottom line

- **Actuator-level parity** (CreateAccountActuator + AccountCapsule defaults) is **mostly correct** for standard inputs (correct prefix, Normal type).
- There are **two concrete semantic mismatches** relative to Java validation/encoding:
  - address prefix strictness
  - ignoring the contract `type`
- If the goal is **full embedded parity** (including bandwidth/create-account resource path and receipt bytes), the current implementation is **not** equivalent to Java’s overall behavior.
