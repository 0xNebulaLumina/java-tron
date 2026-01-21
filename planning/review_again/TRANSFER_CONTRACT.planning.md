# Review: `TRANSFER_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_transfer_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `TransferActuator` in `actuator/src/main/java/org/tron/core/actuator/TransferActuator.java`

Goal: verify whether the Rust implementation matches java-tron’s **validation + state transition** semantics for TRX transfers, and identify mismatches that can affect conformance outputs or consensus state.

Note: java-tron’s *bandwidth accounting* is handled outside the actuator by `BandwidthProcessor` (see `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java`). Rust’s implementation currently mixes some bandwidth/AEXT behavior into the contract executor, so this review calls out those differences explicitly.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`TransferActuator.validate`)

Key checks (and messages) in order:

1. Contract type: `any.is(TransferContract.class)`
   - Error: `contract type error, expected type [TransferContract], real type [class com.google.protobuf.Any]`
2. `DecodeUtil.addressValid(ownerAddress)`
   - **Requires 21 bytes** and **prefix == `DecodeUtil.addressPreFixByte`**
   - Error: `Invalid ownerAddress!`
3. `DecodeUtil.addressValid(toAddress)`
   - Error: `Invalid toAddress!`
4. `toAddress != ownerAddress`
   - Error: `Cannot transfer TRX to yourself.`
5. Owner account exists
   - Error: `Validate TransferContract error, no OwnerAccount.`
6. `amount > 0`
   - Error: `Amount must be greater than 0.`
7. Fee component:
   - `calcFee() == TRANSFER_FEE` (in this repo: `TRANSFER_FEE = 0`)
   - If recipient account is missing: `fee += CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
8. Contract-type restrictions:
   - If `FORBID_TRANSFER_TO_CONTRACT == 1` and recipient exists and `AccountType.Contract`:
     - Error: `Cannot transfer TRX to a smartContract.`
   - If `ALLOW_TVM_COMPATIBLE_EVM == 1` and recipient exists and `AccountType.Contract`:
     - If contract missing from contract store (should not happen): `Account type is Contract, but it is not exist in contract store.`
     - If contract version == 1:
       - Error: `Cannot transfer TRX to a smartContract which version is one. Instead please use TriggerSmartContract `
9. Balance and overflow:
   - If `balance < addExact(amount, fee)`:
     - Error: `Validate TransferContract error, balance is not sufficient.`
   - If recipient exists, check `addExact(toBalance, amount)` overflow:
     - Error: `long overflow`

### 2) Execution (`TransferActuator.execute`)

State transition:

- If recipient is missing:
  - Create `AccountCapsule(toAddress, Normal, create_time=LATEST_BLOCK_HEADER_TIMESTAMP, withDefaultPermission=(ALLOW_MULTI_SIGN==1), dynamicStore)`
  - `fee += CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- Deduct from sender:
  - `owner.balance -= addExact(fee, amount)`
- Fee destination:
  - If `supportBlackHoleOptimization()`: `burnTrx(fee)` (increments `BURN_TRX_AMOUNT`)
  - Else: credit blackhole account balance by `fee`
- Credit recipient:
  - `to.balance += amount`
- Receipt:
  - `ret.setStatus(fee, SUCESS)` (fee includes create-account fee when applicable)

### 3) Bandwidth accounting (outside the actuator)

java-tron consumes bandwidth (and may charge TRX if needed) in `BandwidthProcessor.consume(...)`:

- `bytesSize` is based on protobuf transaction serialization size (not a fixed constant).
- Path selection uses:
  - CREATE_ACCOUNT path (special ratio) when `TransferContract` creates a recipient account (`contractCreateNewAccount`)
  - Otherwise ACCOUNT_NET → FREE_NET → FEE (TRX per byte) based on frozen bandwidth and free net availability.
- Time windowing uses `now = chainBaseManager.getHeadSlot()` (slot derived from block timestamp, not block number).

---

## Rust implementation behavior (what it currently does)

`execute_transfer_contract()`:

- Validates `Any.type_url` ends with `"protocol.TransferContract"` when `metadata.contract_parameter` is present.
- Reads:
  - `owner` from `transaction.from` (20-byte EVM address)
  - `to` from `transaction.to` (20-byte EVM address, `Option`)
  - `amount` from `transaction.value` (U256 low-8-bytes → `i64`)
  - raw owner bytes from `transaction.metadata.from_raw` (as sent by Java)
- Validates (attempting parity with `TransferActuator.validate`):
  - owner address: `from_raw.len() == 21` and `from_raw[0] == storage_adapter.address_prefix()`
  - `to` is present (`transaction.to != None`), else `"Invalid toAddress!"`
  - `to != owner` (compares 20-byte addresses)
  - owner account exists
  - `amount > 0`
  - recipient existence → adds `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
  - forbid-transfer-to-contract and allow-tvm-compatible-evm contract-version checks (via proto account type + contract store)
  - balance and overflow checks (uses `checked_add`, returns `"long overflow"`)
- Executes:
  - deducts sender balance by `amount + create_account_fee + fee_amount`
  - credits recipient balance by `amount`
  - creates recipient proto account when missing:
    - `create_time = LATEST_BLOCK_HEADER_TIMESTAMP`
    - if `ALLOW_MULTI_SIGN == 1`, populates default `owner_permission` and `active_permission` (with `ACTIVE_DEFAULT_OPERATIONS`)
  - burns or credits blackhole for **create-account-fee** based on `support_black_hole_optimization()`
  - optionally applies an extra configured `fee_amount` (`fee_config.non_vm_blackhole_credit_flat`)
- Returns `TronExecutionResult` with:
  - `energy_used = 0`
  - `bandwidth_used = calculate_bandwidth_usage(transaction)`
  - `state_changes` (account deltas; sorted deterministically)
  - optional `aext_map` updates when `accountinfo_aext_mode == "tracked"`

Bandwidth/AEXT details:

- `calculate_bandwidth_usage(...)` is currently a **simplified estimate** (`60 + data_len + 65`) implemented in `rust-backend/crates/core/src/service/contracts/freeze.rs`.
- AEXT tracking uses `ResourceTracker::track_bandwidth(...)` in `rust-backend/crates/execution/src/storage_adapter/resource.rs`, which currently:
  - hardcodes `account_net_limit = 0` (so it never chooses ACCOUNT_NET)
  - uses `now = context.block_number` (while Java uses `headSlot`)
  - does not implement the CREATE_ACCOUNT bandwidth ratio path
  - does not charge TRX per byte for the FEE path (it only returns `BandwidthPath::Fee`)

---

## Does it match java-tron?

### What matches (good parity)

- **Core TRX balance transition** for normal transfers:
  - sender decreases by `amount` (+ create-account-fee when recipient is new)
  - recipient increases by `amount`
- **Create-account semantics (actuator-level)**:
  - charges `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` only when recipient account is absent
  - creates recipient account with `create_time = LATEST_BLOCK_HEADER_TIMESTAMP`
  - populates default permissions when `ALLOW_MULTI_SIGN == 1`
- **Contract restrictions**:
  - `FORBID_TRANSFER_TO_CONTRACT` and `ALLOW_TVM_COMPATIBLE_EVM` checks and messages are aligned with `TransferActuator.validate`.
- **Overflow/error strings**:
  - Rust returns `"long overflow"` in the same situations Java would throw `ArithmeticException("long overflow")`.

### Where it diverges (real mismatches / risk areas)

1) **`toAddress` validation strictness and error ordering do not fully match Java**

- Java requires `toAddress` to be **exactly 21 bytes** and have **prefix == configured prefix** (`DecodeUtil.addressPreFixByte`).
- Rust’s executor does **not** validate `toAddress` raw bytes against `storage_adapter.address_prefix()`; it only checks that `transaction.to` is present.
- Rust’s gRPC conversion (`strip_tron_address_prefix` in `rust-backend/crates/core/src/service/grpc/address.rs`) accepts:
  - 21-byte addresses with prefix `0x41` **or** `0xa0` (not chain-specific)
  - **20-byte** addresses (no prefix)
  - and it fails early (before contract validation) on malformed lengths with an error message that is **not** `"Invalid toAddress!"`.

Impact:

- Wrong-prefix or 20-byte `to` addresses can be accepted by Rust where Java would reject.
- Fixtures or malformed inputs where both owner and to are invalid can produce **different error ordering** (Java: owner invalid first; Rust: conversion can fail on `to` before contract validation runs).

2) **Bandwidth usage / AEXT tracking is not equivalent to java-tron**

- Java bandwidth usage is computed from serialized tx size and uses `headSlot` time windowing; it also has a special CREATE_ACCOUNT bandwidth ratio path for `TransferContract` that creates a recipient.
- Rust uses:
  - an approximate `calculate_bandwidth_usage`
  - a simplified `ResourceTracker` that does not model ACCOUNT_NET, CREATE_ACCOUNT, or TRX-per-byte fee charging.

Impact:

- Remote execution can diverge in:
  - `bandwidth_used` values
  - AEXT/net usage mutations (when tracked)
  - any downstream logic that expects BandwidthProcessor-equivalent behavior (CSV state digest parity, resource billing, etc.).

3) **Optional extra fee handling (`fee_amount`) is not java-tron semantics**

- Java’s `TRANSFER_FEE` is 0 in this repo, and TransferContract fees beyond create-account are normally handled by bandwidth processors (not by the actuator).
- Rust supports an extra configured fee (`fee_config.non_vm_blackhole_credit_flat`) which, if enabled, changes:
  - sender deduction (`amount + create_account_fee + fee_amount`)
  - fee destination behavior (config-driven `burn`/`blackhole`), and in `burn` mode it does **not** call `burn_trx(...)` to update `BURN_TRX_AMOUNT`.

Impact:

- With any non-default fee configuration, Rust will not match java-tron’s state/receipt side effects.

---

## Bottom line

- For **standard, well-formed TRX transfers** under default configuration (no extra fee), Rust’s `TRANSFER_CONTRACT` implementation largely matches java-tron’s **actuator-level** validation and balance/account creation semantics.
- It is **not fully equivalent** to java-tron for:
  - strict `toAddress` validation and error-order parity in malformed cases
  - bandwidth accounting / AEXT tracking parity (which is handled outside the actuator in Java)
  - any configuration that introduces non-zero “flat” fees for non-VM transfers

