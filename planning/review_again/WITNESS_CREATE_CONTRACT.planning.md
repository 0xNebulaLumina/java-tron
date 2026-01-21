# Review: `WITNESS_CREATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_witness_create_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `WitnessCreateActuator` in `actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java`
- **Java→Rust request mapping**: `RemoteExecutionSPI` in `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (how `tx.data` / `tx.from` are populated for remote execution)

Goal: determine whether the Rust implementation matches java-tron’s **validation + state transition** for witness creation, and identify any consensus-/state-relevant mismatches.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`WitnessCreateActuator.validate`)

Key checks (and messages) in order:

1. Contract type: `any.is(WitnessCreateContract.class)`
   - Error: `contract type error, expected type [WitnessCreateContract],real type[class com.google.protobuf.Any]`
2. `DecodeUtil.addressValid(ownerAddress)` (requires 21 bytes and correct prefix)
   - Error: `Invalid address`
3. `TransactionUtil.validUrl(urlBytes)` (non-empty, ≤ 256 bytes)
   - Error: `Invalid url`
4. Owner account exists
   - Error: `account[<hexOwner>] not exists`
5. Witness does not already exist (`witnessStore.has(ownerAddress)`)
   - Error: `Witness[<hexOwner>] has existed`
6. Owner balance ≥ `AccountUpgradeCost`
   - Error: `balance < AccountUpgradeCost`

### 2) Execution (`WitnessCreateActuator.execute` + `createWitness`)

State transition:

- Create witness entry: `WitnessCapsule(ownerAddress, voteCount=0, url.toStringUtf8())` → `witnessStore.put(...)`
- Update owner account:
  - `account.setIsWitness(true)`
  - If `ALLOW_MULTI_SIGN == 1`: `account.setDefaultWitnessPermission(dynamicStore)` which:
    - always sets `witness_permission` (id=1, threshold=1, key=owner weight=1)
    - ensures `owner_permission` exists (id=0, threshold=1)
    - ensures at least one `active_permission` exists (id=2, operations=`ACTIVE_DEFAULT_OPERATIONS`)
- Charge `AccountUpgradeCost`:
  - deduct from owner balance
  - burn (`burnTrx`) if `supportBlackHoleOptimization()`, else credit the blackhole account
- Update totals: `addTotalCreateWitnessCost(cost)`

---

## Rust implementation behavior (what it currently does)

`execute_witness_create_contract()`:

- Validates contract type using `metadata.contract_parameter.type_url` (when provided).
- Validates owner address using `metadata.from_raw` (21 bytes + `storage_adapter.address_prefix()`).
- Interprets `transaction.data` as the URL bytes; validates non-empty and ≤ 256 bytes; decodes via `String::from_utf8_lossy(...)`.
- Requires owner EVM account exists (`get_account`) and witness does not exist (`is_witness`).
- Requires owner balance ≥ `AccountUpgradeCost`.
- Writes:
  - witness entry via `storage_adapter.put_witness(WitnessInfo{ vote_count: 0, url, ... })`
  - owner `Account` proto: sets `is_witness = true` and persists via `put_account_proto`
  - owner balance update via `set_account(...)` (deducting `AccountUpgradeCost`)
  - burn/blackhole credit depending on `support_black_hole_optimization()`
  - `TOTAL_CREATE_WITNESS_FEE` via `add_total_create_witness_cost`
- Returns a `TronExecutionResult` with `energy_used=0`, `bandwidth_used>0`, plus account `state_changes` for owner/blackhole balance deltas.

**Important wiring note**: Java remote execution builds the request with `tx.data = WitnessCreateContract.url` bytes (not the full protobuf), and passes the original `Any` in `tx.contract_parameter`. This matches Rust’s expectation that `transaction.data` is the URL payload.

---

## Does it match java-tron?

### What matches (good parity)

- **Validation semantics and messages**: address validity, URL length/emptiness, account existence, witness existence, and `balance < AccountUpgradeCost` align with `WitnessCreateActuatorTest`.
- **Core state transition**:
  - creates a witness entry with `vote_count = 0` and the URL decoded as UTF-8 (replacement on invalid sequences is consistent with Java’s `toStringUtf8()` behavior)
  - sets `Account.is_witness = true`
  - charges `AccountUpgradeCost`, then burns or credits blackhole based on `supportBlackHoleOptimization()`
  - increments `TOTAL_CREATE_WITNESS_FEE`

### Where it diverges (real mismatch)

1) **Missing default witness permissions when `ALLOW_MULTI_SIGN == 1` (state mismatch)**

Java explicitly does:

- `if (dynamicStore.getAllowMultiSign() == 1) { account.setDefaultWitnessPermission(dynamicStore); }`

Rust fetches `allow_multi_sign` but does not apply the equivalent update to the owner `Account` proto:

- no `witness_permission` initialization
- no “ensure owner permission exists” / “ensure active permission exists” behavior

Impact:

- With `ALLOW_MULTI_SIGN` enabled (commonly `1` in practice), remote execution produces an owner `Account` state that differs from embedded execution, and can change downstream permission/signature behavior (and any byte-level conformance that inspects account protos).

### Likely worth double-checking (not unique to witness-create, but triggered here)

2) **Account proto byte-level encoding risks when balance is updated**

`execute_witness_create_contract()` performs:

1. `put_account_proto(...)` (writes java-compatible bytes, including map-order rewrites for `asset_v2` when needed)
2. `set_account(...)` (updates balance via `serialize_account_update`, which re-encodes the Account proto via prost without the java-compat rewrite step)

If the owner account has a non-trivial `asset_v2` map (≥2 entries, empty keys, or zero values), `set_account(...)` may re-encode it with different ordering/default-field behavior than java-tron.

This is broader than witness creation, but witness-create does update balance, so it can surface here.

---

## Bottom line

Rust’s `WITNESS_CREATE_CONTRACT` execution is close to java-tron for **validation** and the **basic economic state transition**, but it is **not fully equivalent** while `ALLOW_MULTI_SIGN == 1` because it does not replicate `AccountCapsule.setDefaultWitnessPermission(...)` behavior.

