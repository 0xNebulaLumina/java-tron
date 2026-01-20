# Review: `TRANSFER_ASSET_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_trc10_transfer_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `TransferAssetActuator` in `actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java`

Goal: confirm whether the Rust implementation matches java-tron’s **validation + state transition** for TRC-10 transfers, and call out mismatches that can change consensus state or conformance outputs.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`TransferAssetActuator.validate`)

Key checks (and messages) in order:

1. Contract type: `any.is(TransferAssetContract.class)`
   - Error: `contract type error, expected type [TransferAssetContract], real type[class com.google.protobuf.Any]`
2. `DecodeUtil.addressValid(ownerAddress)`
   - **Requires 21 bytes** and **prefix == DecodeUtil.addressPreFixByte**
   - Error: `Invalid ownerAddress`
3. `DecodeUtil.addressValid(toAddress)`
   - Error: `Invalid toAddress`
4. `amount > 0`
   - Error: `Amount must be greater than 0.`
5. `ownerAddress != toAddress`
   - Error: `Cannot transfer asset to yourself.`
6. Owner account exists
   - Error: `No owner account!`
7. Asset exists in the correct store (V1 vs V2 via `ALLOW_SAME_TOKEN_NAME`)
   - Error: `No asset!`
8. Owner TRC-10 balance:
   - missing/<=0: `assetBalance must be greater than 0.`
   - insufficient: `assetBalance is not sufficient.`
9. Recipient handling:
   - if recipient exists:
     - if `FORBID_TRANSFER_TO_CONTRACT == 1` and recipient is `AccountType.Contract`:
       - Error: `Cannot transfer asset to smartContract.`
     - if recipient already has a balance entry, check `addExact(balance, amount)` overflow
   - else (recipient absent):
     - require owner TRX balance ≥ `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
     - Error: `Validate TransferAssetActuator error, insufficient fee.`

### 2) Execution (`TransferAssetActuator.execute`)

State transition:

- If recipient account is missing:
  - Create `AccountCapsule(toAddress, Normal, createTime, withDefaultPermission, dynamicStore)`
  - `withDefaultPermission = (ALLOW_MULTI_SIGN == 1)`:
    - when true, java-tron initializes default `owner_permission` and `active_permission` (with `ACTIVE_DEFAULT_OPERATIONS`)
  - Fee increases by `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- Update TRC-10 balances using:
  - `owner.reduceAssetAmountV2(assetNameBytes, amount, dynamicStore, assetIssueStore)`
  - `recipient.addAssetAmountV2(assetNameBytes, amount, dynamicStore, assetIssueStore)`
  - In legacy mode (`ALLOW_SAME_TOKEN_NAME == 0`), both `Account.asset[name]` and `Account.assetV2[tokenId]` are updated.
- Deduct TRX fee from owner balance (only the create-account fee; `calcFee() == 0`)
- Fee destination:
  - if `supportBlackHoleOptimization()`: burn TRX (`burnTrx`)
  - else: credit blackhole account balance

---

## Rust implementation behavior (what it currently does)

`execute_trc10_transfer_contract()`:

- Validates `Any.type_url == "protocol.TransferAssetContract"` when available.
- Validates owner address via `transaction.metadata.from_raw` (but **does not** use the configured prefix).
- Reads:
  - `owner` from `transaction.from`
  - `to` from `transaction.to`
  - `amount` from `transaction.value` (U256 → i64-ish path)
  - `asset_id` from `transaction.metadata.asset_id` (derived from Java’s `assetId` field)
- Validates:
  - amount > 0
  - owner != to
  - owner account proto exists
  - asset exists in V1/V2 store depending on `ALLOW_SAME_TOKEN_NAME`
  - owner has sufficient TRC-10 balance
  - recipient contract check (when recipient exists) against `FORBID_TRANSFER_TO_CONTRACT`
  - overflow check for recipient balance (if present)
  - create-account-fee affordability when recipient missing
- Executes:
  - creates recipient **proto** account when missing (type Normal, create_time set)
  - updates TRC-10 balances via `reduce_asset_amount_v2` / `add_asset_amount_v2`
  - deducts create-account fee from owner proto balance
  - burns or credits blackhole for create-account fee
- Builds a `TronExecutionResult` with:
  - `energy_used = 0`, `bandwidth_used > 0`
  - `trc10_changes = [AssetTransferred(...)]`
  - account-level `state_changes` primarily for AEXT / CSV parity (often no-op deltas)

---

## Does it match java-tron?

### What matches (good parity)

- **Core validation set**: amount positivity, self-transfer prohibition, owner existence, asset existence, owner TRC-10 balance checks, forbid-transfer-to-contract check, and recipient overflow check align with `TransferAssetActuator.validate`.
- **TRC-10 balance semantics**:
  - respects `ALLOW_SAME_TOKEN_NAME` and updates `asset` + `asset_v2` appropriately in legacy mode (matching `AccountCapsule.{add,reduce}AssetAmountV2`).
- **Create-account fee semantics**:
  - charges `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` only when recipient is missing
  - burns vs credits blackhole based on `supportBlackHoleOptimization()`

### Where it diverges (real mismatches / risk areas)

1) **Recipient default permissions when `ALLOW_MULTI_SIGN == 1` (consensus-relevant)**

Java creates the recipient account with `withDefaultPermission = (ALLOW_MULTI_SIGN == 1)` and initializes:

- `owner_permission` (id=0, threshold=1, key=address weight=1)
- `active_permission` (id=2, threshold=1, operations=`ACTIVE_DEFAULT_OPERATIONS`)

Rust currently creates a minimal `ProtoAccount { address, create_time, type=Normal, ..Default }` and does **not** populate these permission fields.

Impact:

- When `ALLOW_MULTI_SIGN` is enabled (mainnet / many networks), Rust-created accounts from TRC-10 transfers will differ from java-tron’s state (missing permissions), potentially breaking downstream signature/permission behavior and conformance byte-level equality.

2) **Address validation strictness does not match `DecodeUtil.addressValid`**

- Java requires **21 bytes** and **prefix == configured network prefix**.
- Rust owner validation currently accepts:
  - 21 bytes with prefix `0x41` **or** `0xa0`
  - **or** 20-byte addresses as “valid”

Impact:

- Wrong-prefix and 20-byte owner addresses can pass Rust validation where Java would fail with `Invalid ownerAddress`.
- The check should likely use `storage_adapter.address_prefix()` (like Rust’s `TRANSFER_CONTRACT` implementation does) and require 21-byte addresses for parity.

3) **Empty `asset_name` handling differs (error message parity)**

- Java treats empty `asset_name` as “missing asset” and fails with `No asset!` (via store `has()`).
- Rust requires `metadata.asset_id` to be present; Java’s remote mapping omits `asset_id` when empty, so Rust fails earlier with `asset_id is required for TransferAssetContract`.

Impact:

- Edge-case validation errors diverge from java-tron.
- This is likely fixable either by:
  - parsing `TransferAssetContract` from `contract_parameter.value` (already provided) and using its bytes, or
  - carrying `asset_id` even when empty and mapping the failure to `No asset!`.

4) **Returned `state_changes` don’t obviously reflect create-account-fee deltas**

Java’s execution mutates:

- owner TRX balance (deduct create-account fee)
- blackhole account balance (or burn counter)

Rust persists those changes to the underlying stores, but its emitted `state_changes` are primarily no-op AccountChanges for CSV/AEXT parity and do not obviously include:

- an owner balance delta for create-account fee
- a blackhole account delta when crediting

Impact:

- If conformance / CSV reporting expects these account deltas to appear in `state_changes`, Rust will diverge even if persisted DB state is correct.

---

## Bottom line

- For **TRC-10 balance movement** and core validations, Rust is very close to `TransferAssetActuator`.
- It is **not fully equivalent** to java-tron in at least two important areas:
  - **Account creation permissions** when `ALLOW_MULTI_SIGN == 1` (state mismatch)
  - **Address validation strictness** (potentially different acceptance / error behavior)
- There are also parity gaps in edge-case error messages and in the `state_changes` representation used for conformance/reporting.

