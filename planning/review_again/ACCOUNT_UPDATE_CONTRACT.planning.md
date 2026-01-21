# Review: `ACCOUNT_UPDATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_account_update_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `UpdateAccountActuator` in `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`

It focuses on whether the Rust implementation matches **java-tron’s validation + execution semantics** for `AccountUpdateContract` (type 10).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`UpdateAccountActuator.validate`)

Sources:

- `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`
- `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountName`)
- `common/src/main/java/org/tron/common/utils/DecodeUtil.java` (`addressValid`)
- `chainbase/src/main/java/org/tron/core/store/AccountIndexStore.java` (`has`)
- `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java` (`getAllowUpdateAccountName`)

Order and rules:

1. Rejects wrong type: `any.is(AccountUpdateContract.class)`; error string:
   - `contract type error, expected type [AccountUpdateContract], real type[class com.google.protobuf.Any]`
2. Unpacks contract: `any.unpack(AccountUpdateContract.class)`
3. Validates `account_name` via `TransactionUtil.validAccountName(bytes)`:
   - **allows empty**
   - **max len = 200 bytes**
4. Validates `owner_address` via `DecodeUtil.addressValid(bytes)`:
   - **requires 21 bytes**
   - **requires prefix byte == `DecodeUtil.addressPreFixByte`** (mainnet `0x41`, testnet `0xa0`)
5. Requires owner account exists: `AccountStore.get(ownerAddress) != null`
6. If `ALLOW_UPDATE_ACCOUNT_NAME == 0`:
   - rejects if the account already has a non-empty name: `This account name is already existed`
   - rejects if `AccountIndexStore.has(accountName)` is true: `This name is existed`

### 2) Execution (`UpdateAccountActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`

- Fee is always `0` (`calcFee() == 0`)
- Applies:
  - `account.setAccountName(accountNameBytes)`
  - `AccountStore.put(ownerAddress, account)`
  - `AccountIndexStore.put(account)` (writes `name -> address`, overwriting on duplicates; does **not** remove the old-name key)

### 3) Remote execution request shaping (important coupling)

Source: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

For `AccountUpdateContract` the Java remote request mapping is:

- `from = owner_address` (raw bytes, 21B w/ prefix)
- `to = empty`
- `data = account_name` (raw bytes)
- `contract_type = ACCOUNT_UPDATE_CONTRACT`
- `contract_parameter = original Any` (type_url + value)

This is relevant because the Rust backend implementation reads `transaction.data` as the name bytes.

---

## Rust implementation behavior (what it currently does)

Sources:

- `rust-backend/crates/core/src/service/mod.rs` (`execute_account_update_contract`)
- `rust-backend/crates/execution/src/storage_adapter/engine.rs`
  - `get_allow_update_account_name`
  - `account_index_has`
  - `set_account_name`

### Validation & parsing

- If `transaction.metadata.contract_parameter` is present:
  - checks Any `type_url` tail matches `protocol.AccountUpdateContract`
  - returns the Java-matching error string on mismatch
- Uses:
  - `name_bytes = transaction.data`
  - owner address validity check based on `transaction.metadata.from_raw`:
    - currently accepts:
      - `len == 21` with prefix `0x41` **or** `0xa0`
      - `len == 20` (treated as valid)
    - errors with `Invalid ownerAddress` otherwise
- Validates account name by length only:
  - rejects if `name_bytes.len() > 200` with `Invalid accountName`
- Requires owner account exists:
  - `storage_adapter.get_account(transaction.from)` must be `Some(...)` (same underlying Account DB)
- Fetches `ALLOW_UPDATE_ACCOUNT_NAME` from dynamic properties:
  - if `== 0` and owner already has a non-empty name: `This account name is already existed`
  - if `== 0` and `account-index` already has `name_bytes` as key: `This name is existed`

### State updates

- Calls `storage_adapter.set_account_name(transaction.from, name_bytes)` which:
  - updates `Account.account_name` in the `account` DB
  - writes `account-index[name] = owner_tron_address_21` (overwrite semantics; no old-name cleanup)

### Execution result

- Returns `energy_used = 0`
- Returns `bandwidth_used = calculate_bandwidth_usage(transaction)` (simplified estimate, not full TRON tx serialization)
- Emits **one** `TronStateChange::AccountChange` for the owner with `old_account == new_account`

---

## Does it match java-tron?

### What matches (good parity)

- **Account name validation**: Java allows empty + `len <= 200`; Rust implements the same constraint.
- **Dynamic-property gating**:
  - only-set-once behavior when `ALLOW_UPDATE_ACCOUNT_NAME == 0`
  - duplicate-name check using account-index only when updates are disabled
- **Error strings** for the above validate-fail cases match Java exactly:
  - `Invalid accountName`
  - `Invalid ownerAddress`
  - `Account does not exist`
  - `This account name is already existed`
  - `This name is existed`
- **Storage writes** match Java’s stores:
  - account proto field `account_name` updated
  - account-index updated as `name -> address` with overwrite semantics and no old-name cleanup
- **Fee**: always `0` (energy used 0; Java actuator fee 0)

### Where it diverges / parity risks

1) **Owner address validation is looser than Java**

- Java (`DecodeUtil.addressValid`) requires:
  - `len == 21`
  - prefix byte equals the *configured* `DecodeUtil.addressPreFixByte`
- Rust currently accepts:
  - `len == 21` with either `0x41` or `0xa0` regardless of configured prefix
  - `len == 20` as valid

Impact:

- A wrong-prefix address (e.g., `0xa0` on mainnet) would be rejected in Java as `Invalid ownerAddress`, but can pass Rust validation and fail later as `Account does not exist` (different error + ordering).
- Any fixture/client using 20-byte owner addresses (missing TRON prefix) would fail validation in Java but may pass in Rust.

2) **Rust does not unpack `contract_parameter.value` as `AccountUpdateContract`**

- Java derives `owner_address` and `account_name` from `any.unpack(AccountUpdateContract.class)`.
- Rust only checks the Any type URL, then trusts:
  - `transaction.from` (+ `from_raw`) for owner
  - `transaction.data` for name bytes

Impact:

- If a caller sends inconsistent `contract_parameter` vs `from/data`, Rust and Java can diverge.
- If conformance ever includes malformed Any `value` bytes that Java would reject on unpack, Rust may not surface the same error.

3) **Bandwidth accounting is approximate**

- Java bandwidth is based on full TRON tx serialized size and resource rules.
- Rust uses a simplified byte estimate (`base + data_len + signature_len`).

Impact:

- `bandwidth_used` will not match embedded semantics if you compare receipts/resources directly (may be fine if Java remains authoritative for bandwidth/receipt accounting in remote mode).

---

## Bottom line

- **Actuator semantics and storage side effects** for `ACCOUNT_UPDATE_CONTRACT` are **substantially aligned** with java-tron *given the current RemoteExecutionSPI mapping* (`data = account_name bytes`).
- There are still **two real parity gaps** worth fixing if strict conformance is required:
  1) owner address validation should match `DecodeUtil.addressValid` (prefix strictness + 21-byte requirement)
  2) contract parameter unpack parity (optional, but improves robustness and matches the stated design intent of `contract_parameter`)

