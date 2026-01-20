# Review: `UNFREEZE_ASSET_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_unfreeze_asset_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `UnfreezeAssetActuator` in `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`

It focuses on whether Rust matches java-tron’s actuator semantics (validation + state changes) for TRC-10 `UnfreezeAssetContract` (contract type 14).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`UnfreezeAssetActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`

Key checks (in order):

1. Contract parameter `Any` is present and `any.is(UnfreezeAssetContract.class)`:
   - else: `"contract type error, expected type [UnfreezeAssetContract], real type[class com.google.protobuf.Any]"`
2. Unpack `UnfreezeAssetContract`, read `ownerAddress`
3. `DecodeUtil.addressValid(ownerAddress)` (non-empty, length 21, correct prefix) else `"Invalid address"`
4. Owner account exists else `"Account[<hex(ownerAddress)>] does not exist"`
5. `account.frozenSupplyCount > 0` else `"no frozen supply balance"`
6. Issuer linkage:
   - if `ALLOW_SAME_TOKEN_NAME == 0`: `assetIssuedName` must be non-empty else `"this account has not issued any asset"`
   - else: `assetIssuedID` must be non-empty else `"this account has not issued any asset"`
7. Time gate:
   - `allowedUnfreezeCount = count(frozenSupply.expireTime <= now)` must be `> 0`
   - else: `"It's not time to unfreeze asset supply"`

### 2) Execution (`UnfreezeAssetActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`

State transitions:

1. `fee = 0`
2. Copy `account.frozenSupplyList` and remove all entries with `expireTime <= now`, accumulating:
   - `unfreezeAsset += frozenBalance` (note: **unchecked** `long` addition)
3. Add the sum back to issuer’s TRC-10 balance via `account.addAssetAmountV2(...)`:
   - if `ALLOW_SAME_TOKEN_NAME == 0`: key is `assetIssuedName` and `addAssetAmountV2` updates both `Account.asset[name]` and `Account.assetV2[tokenId]`
   - else: key is `assetIssuedID` and `addAssetAmountV2` updates `Account.assetV2[tokenId]` only
4. Persist updated account with remaining `frozenSupplyList`

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

`execute_unfreeze_asset_contract()` performs validation + execution in one function:

1. Owner address validation:
   - uses `transaction.metadata.from_raw` (expects 21 bytes and correct DB prefix) else `"Invalid address"`
2. Loads owner account by address; missing account errors: `"Account[<hex>] does not exist"`
3. Requires `account.frozen_supply` non-empty else `"no frozen supply balance"`
4. Reads `ALLOW_SAME_TOKEN_NAME` and chooses `asset_key` from:
   - `account.asset_issued_name` if `0`, else `account.asset_issued_id`
   - missing key errors: `"this account has not issued any asset"`
5. Loads `asset_issue` via `storage_adapter.get_asset_issue(asset_key, allowSameTokenName)`; missing errors: `"No asset!"`
6. Time gate: counts expired frozen entries and errors if `0`: `"It's not time to unfreeze asset supply"`
7. Builds a new frozen list, summing expired balances using `checked_add` (overflow → `"Overflow calculating unfreeze amount"`)
8. Applies the unfreeze by updating the issuer’s TRC-10 map via `add_asset_amount_v2(...)`
9. Persists updated account proto via `put_account_proto`
10. Returns `TronExecutionResult` with:
    - `state_changes`: an `AccountChange` where TRX balance is unchanged
    - `trc10_changes`: **empty**

---

## Does it match java-tron?

### What matches (good parity vs `UnfreezeAssetActuator`)

- **Core unfreeze semantics**: Rust removes all expired `frozen_supply` entries and credits the sum back into issuer TRC-10 balance, matching Java’s behavior.
- **Main validation checks**: invalid address, missing account, empty frozen supply, missing `assetIssuedName/assetIssuedID`, and “not time yet” are all present with the same primary strings as Java.
- **ALLOW_SAME_TOKEN_NAME split**: Rust uses `asset_issued_name` vs `asset_issued_id` and updates `asset`/`asset_v2` similarly to Java’s `addAssetAmountV2` intent.

### Where it diverges (real mismatches / risk areas)

1) **Missing `Any.is(UnfreezeAssetContract)` contract-type validation**

- Java fails early if `Any` does not match the expected contract type.
- Rust does not check `transaction.metadata.contract_parameter.type_url` at all for this contract.

Impact:

- If the upstream mapping ever provides a wrong `contract_parameter`, Rust can proceed (and fail later with a different error), breaking java-tron parity for type-check/order and potentially masking “wrong contract” issues.

2) **Owner address source differs (Rust uses `from_raw`, Java uses unpacked contract field)**

- Java uses `unfreezeAssetContract.getOwnerAddress()`.
- Rust uses `transaction.metadata.from_raw` and never parses the contract bytes for owner.

Impact:

- In “well-formed” transactions this is equivalent (RemoteExecutionSPI sets `from` from `trxCap.getOwnerAddress()`).
- If `from_raw` and the embedded contract owner ever diverge due to malformed data or mapping bugs, Rust and Java can disagree on which address is validated/used.

3) **Rust is stricter than Java on overflow**

- Java sums `unfreezeAsset += frozenBalance` without overflow checking (wraps on overflow).
- Rust uses `checked_add` and aborts on overflow (`"Overflow calculating unfreeze amount"`).

Impact:

- Likely irrelevant on valid chains (sum of frozen balances should be within `long`), but it is not byte-for-byte parity for corrupted/extreme states.

4) **Rust requires `AssetIssue` lookup even when Java doesn’t**

- Java validate does **not** consult `AssetIssueStore` at all.
- Java execute consults `AssetIssueStore` only inside `addAssetAmountV2` when `ALLOW_SAME_TOKEN_NAME == 0` (to map name → tokenId).
- Rust always calls `get_asset_issue(...)` and fails early with `"No asset!"` even in the `ALLOW_SAME_TOKEN_NAME == 1` path where Java doesn’t need the store.

Impact:

- Potentially different failure mode and error ordering if the asset-issue entry is missing/corrupt, especially in the `ALLOW_SAME_TOKEN_NAME == 1` path.

5) **No “apply” signal for Java in Phase A (compute-only) remote execution**

- Rust returns `trc10_changes: vec![]` and `state_changes` only cover TRX `AccountInfo`.
- Java’s `RuntimeSpiImpl.applyTrc10Changes(...)` only knows how to apply `AssetIssued` and `AssetTransferred` changes; there is no UnfreezeAsset change type today.

Impact:

- In **WriteMode.COMPUTE_ONLY** (the default on Java side), Java has no explicit instruction to:
  - remove `frozenSupply` entries, or
  - credit the issuer’s TRC-10 balance,
  - so the Java DB can drift from Rust’s shadow DB if UnfreezeAsset remote execution is enabled.

---

## Bottom line

The Rust unfreeze algorithm largely matches Java’s actuator logic for normal, well-formed states, but it is **not fully parity-identical**:

- It misses Java’s `Any` type validation,
- it can differ in error ordering / strictness (asset lookup, overflow),
- and in compute-only remote mode it does not emit a change signal that Java can apply for UnfreezeAsset state transitions.

If we want production-level parity, this contract likely needs follow-up work (see TODO plan).
