# Review: `UPDATE_ASSET_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_update_asset_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `UpdateAssetActuator` in `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`

It focuses on whether Rust matches java-tron’s actuator semantics (validation + state writes) for TRC-10 `UpdateAssetContract` (contract type 15).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`UpdateAssetActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`

Key checks (in order):

1. Contract parameter `Any` exists and `any.is(UpdateAssetContract.class)`:
   - else: `"contract type error, expected type [UpdateAssetContract],real type[...]"` (note: no space after comma)
2. Unpack `UpdateAssetContract`, read:
   - `ownerAddress`, `newLimit`, `newPublicLimit`, `url`, `description`
3. `DecodeUtil.addressValid(ownerAddress)` (must be 21 bytes and match `DecodeUtil.addressPreFixByte`) else `"Invalid ownerAddress"`
4. Owner account exists else `"Account does not exist"`
5. Issuer linkage + store existence:
   - if `ALLOW_SAME_TOKEN_NAME == 0`:
     - `account.assetIssuedName` non-empty else `"Account has not issued any asset"`
     - `AssetIssueStore.get(assetIssuedName)` exists else `"Asset is not existed in AssetIssueStore"`
   - else:
     - `account.assetIssuedID` non-empty else `"Account has not issued any asset"`
     - `AssetIssueV2Store.get(assetIssuedID)` exists else `"Asset is not existed in AssetIssueV2Store"`
6. `TransactionUtil.validUrl(url)` else `"Invalid url"`
   - semantics: non-empty and length `<= 256`
7. `TransactionUtil.validAssetDescription(description)` else `"Invalid description"`
   - semantics: length `<= 200` (empty allowed)
8. Limits:
   - if `newLimit < 0 || newLimit >= oneDayNetLimit` → `"Invalid FreeAssetNetLimit"`
   - if `newPublicLimit < 0 || newPublicLimit >= oneDayNetLimit` → `"Invalid PublicFreeAssetNetLimit"`

### 2) Execution (`UpdateAssetActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`

State transitions:

1. `fee = 0`
2. Load owner `AccountCapsule` by `ownerAddress`
3. Load `AssetIssueCapsuleV2` from `AssetIssueV2Store` by `account.assetIssuedID`
4. Update **only** these fields on the V2 capsule:
   - `free_asset_net_limit`
   - `public_free_asset_net_limit`
   - `url`
   - `description`
5. If `ALLOW_SAME_TOKEN_NAME == 0`:
   - Load legacy `AssetIssueCapsule` from `AssetIssueStore` by `account.assetIssuedName`
   - Update the same four fields on the legacy capsule
   - Persist both:
     - `AssetIssueStore.put(assetIssueCapsule.createDbKey(), assetIssueCapsule)`
     - `AssetIssueV2Store.put(assetIssueCapsuleV2.createDbV2Key(), assetIssueCapsuleV2)`
6. Else (V2 mode):
   - Persist only V2:
     - `AssetIssueV2Store.put(assetIssueCapsuleV2.createDbV2Key(), assetIssueCapsuleV2)`

Important nuance:

- Java **updates each store’s entry in-place** and therefore preserves all other fields in each entry independently (e.g. `public_free_asset_net_usage`, `public_latest_free_net_time`, etc.).

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

`execute_update_asset_contract()` does validation + execution in one function:

1. Optional `Any` contract-type validation:
   - if `transaction.metadata.contract_parameter` exists:
     - enforce `any_type_url_matches(type_url, "protocol.UpdateAssetContract")`
     - on mismatch: returns
       - `"contract type error, expected type [UpdateAssetContract],real type[class com.google.protobuf.Any]"`
2. Parse contract bytes via `parse_update_asset_contract(...)`:
   - reads `description`, `url`, `new_limit`, `new_public_limit`
   - **skips** field `owner_address` and uses `transaction.from`/`metadata.from_raw` instead
3. Owner address validation:
   - uses `transaction.metadata.from_raw`
   - accepts:
     - 21-byte addresses with prefix `0x41` or `0xa0`, OR
     - 20-byte addresses (accepted as valid)
   - else: `"Invalid ownerAddress"`
4. Load owner account by `transaction.from` (EVM 20-byte address) else `"Account does not exist"`
5. Read `ALLOW_SAME_TOKEN_NAME` via `storage_adapter.get_allow_same_token_name()` (defaults to `0` if missing)
6. Determine `asset_key`:
   - `allowSameTokenName == 0`: `account.asset_issued_name` (else `"Account has not issued any asset"`)
   - `allowSameTokenName != 0`: `account.asset_issued_id` (else `"Account has not issued any asset"`)
7. Validate URL + description:
   - `valid_url`: non-empty and `<= 256`
   - `valid_asset_description`: `<= 200`
8. Validate limits against `storage_adapter.get_one_day_net_limit()`:
   - errors match Java:
     - `"Invalid FreeAssetNetLimit"`
     - `"Invalid PublicFreeAssetNetLimit"`
9. Load a single `asset_issue` entry from whichever store corresponds to `allowSameTokenName`:
   - missing errors match Java store names:
     - `"Asset is not existed in AssetIssueStore"` or `"Asset is not existed in AssetIssueV2Store"`
10. Clone the loaded `asset_issue` into `updated_asset`, then overwrite the same four fields:
    - `free_asset_net_limit`, `public_free_asset_net_limit`, `url`, `description`
11. Persist:
    - if `allowSameTokenName == 0`:
      - write legacy store by name key
      - write V2 store by id key **only if** `account.asset_issued_id` is non-empty
    - else:
      - write V2 store by id key

---

## Does it match java-tron?

### What matches (good parity)

- **High-level validations**: account existence, issuer linkage checks, URL/description length semantics, and limit bounds are all implemented with the same primary error strings.
- **Contract-type validation**: when `contract_parameter` (Any) is present, the type check and error string match the Java actuator’s `any.is(...)` failure format.
- **Store write intent**:
  - `ALLOW_SAME_TOKEN_NAME == 1`: update only `asset-issue-v2` (matches Java)
  - `ALLOW_SAME_TOKEN_NAME == 0`: update legacy + v2 stores (matches Java’s intent)

### Where it diverges (real parity gaps / edge-case mismatches)

1) **Validation order differs (error precedence can differ)**

- Java checks asset store existence **before** validating URL/description/limits.
- Rust validates URL/description/limits **before** checking whether the asset exists in the relevant store.

Impact:

- For combined-bad inputs (e.g., missing asset + invalid URL), Java returns `"Asset is not existed in ...Store"`, while Rust returns `"Invalid url"` first.
- This can matter for strict fixture parity where error ordering is asserted.

2) **Owner-address source differs (contract field vs request “from”)**

- Java uses `UpdateAssetContract.owner_address` for validation + account lookup.
- Rust ignores `owner_address` inside the contract bytes and uses `transaction.from` + `metadata.from_raw`.

Impact:

- For well-formed transactions this is equivalent (RemoteExecutionSPI sets `from` from `trxCap.getOwnerAddress()`), but Rust does not detect a mismatch if mapping bugs ever produce inconsistent `from` vs embedded `owner_address`.

3) **Address validity semantics are looser than Java**

- Java `DecodeUtil.addressValid` requires:
  - length exactly 21 bytes
  - prefix equals `DecodeUtil.addressPreFixByte` (configurable network prefix)
- Rust accepts 20-byte “addresses” as valid and only checks for prefixes `0x41` or `0xa0` when length is 21.

Impact:

- With malformed request inputs (especially conformance fixtures), Rust can accept cases Java would reject.

4) **Dynamic property fallback for `ONE_DAY_NET_LIMIT` does not match Java**

- Java initializes `ONE_DAY_NET_LIMIT` default to **`57_600_000_000`** in `DynamicPropertiesStore`.
- Rust’s `get_one_day_net_limit()` fallback default is **`8_640_000_000`**.

Impact:

- If the key is absent (e.g., minimal fixtures, corrupted DB, or early bootstrap), Rust’s limit validation can disagree with Java (rejecting values Java would accept).

5) **When updating both asset stores, Rust writes one cloned proto into both (Java updates each independently)**

- Java:
  - loads legacy entry and V2 entry separately
  - updates only the four fields on each
  - preserves each store entry’s other fields independently
- Rust:
  - loads only one entry (based on `allowSameTokenName`)
  - clones it and writes that full object back to the other store (when `allowSameTokenName == 0`)

Impact:

- If legacy and V2 entries ever diverge in other fields (usage counters, timestamps, etc.), Rust can overwrite the V2 entry’s non-updated fields with legacy values (or vice versa if the load strategy changes), which is not what Java does.

6) **V2-store update in legacy mode is conditional in Rust**

- Java always uses `account.assetIssuedID` in execute (implicitly assumes it is present and that the V2 entry exists).
- Rust only writes the V2 entry in legacy mode if `account.asset_issued_id` is non-empty.

Impact:

- In inconsistent states where `assetIssuedName` exists but `assetIssuedID` is empty, Java would likely error/crash, while Rust may silently update only legacy state.

---

## Bottom line

Rust’s `UPDATE_ASSET_CONTRACT` implementation is very close to Java at the “happy-path” level, but it is **not fully parity-identical**:

- Error precedence differs due to validation ordering.
- Owner-address and address-validity semantics are not identical.
- The `ONE_DAY_NET_LIMIT` fallback default is wrong vs Java.
- In legacy mode, Rust can overwrite the V2 store entry wholesale (instead of preserving non-updated fields as Java does).

If strict java-tron parity is required (especially for conformance fixtures and replay), these should be addressed.

