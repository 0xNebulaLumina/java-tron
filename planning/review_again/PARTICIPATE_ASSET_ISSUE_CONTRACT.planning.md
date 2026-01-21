# Review: `PARTICIPATE_ASSET_ISSUE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_participate_asset_issue_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `ParticipateAssetIssueActuator` in `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`

It focuses on whether Rust matches java-tron’s actuator semantics (validation + state changes) for TRC-10 ParticipateAssetIssue.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`ParticipateAssetIssueActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`

Key checks (in order):

1. Contract type is `ParticipateAssetIssueContract`
2. `DecodeUtil.addressValid(ownerAddress)` → **must be a valid 21-byte TRON address with correct prefix**
3. `DecodeUtil.addressValid(toAddress)` → same strict check
4. `amount > 0` else `"Amount must greater than 0!"`
5. `ownerAddress != toAddress` else `"Cannot participate asset Issue yourself !"`
6. Owner account exists else `"Account does not exist!"`
7. Owner balance `>= amount + fee` (fee is 0) else `"No enough balance !"`
8. Asset exists in `Commons.getAssetIssueStoreFinal(...)` keyed by `asset_name` bytes else `"No asset named <ByteArray.toStr(assetName)>"`
9. `toAddress == assetIssue.owner_address` else `"The asset is not issued by <toAddressHex>"`
10. `now` within `[start_time, end_time)` else `"No longer valid period!"`
11. `exchangeAmount = floorDiv(multiplyExact(amount, num), trxNum)` and `exchangeAmount > 0` else `"Can not process the exchange!"`
12. To account exists else `"To account does not exist!"`
13. Issuer (toAccount) has enough tokens via `toAccount.assetBalanceEnoughV2(assetName, exchangeAmount, dynamicStore)` else `"Asset balance is not enough !"`

### 2) Execution (`ParticipateAssetIssueActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`

State transitions:

1. `fee = 0`
2. Owner balance decreases by `amount`
3. `exchangeAmount = floorDiv(amount * num, trxNum)`
4. Owner receives `exchangeAmount` TRC-10 tokens via `ownerAccount.addAssetAmountV2(assetNameKey, exchangeAmount, dynamicStore, assetIssueStore)`
   - If `ALLOW_SAME_TOKEN_NAME == 0`: updates **both** `Account.asset[name]` and `Account.assetV2[tokenId]`
   - If `ALLOW_SAME_TOKEN_NAME == 1`: updates `Account.assetV2[tokenId]` only
5. Issuer (toAccount) balance increases by `amount`
6. Issuer token balance decreases by `exchangeAmount` via `toAccount.reduceAssetAmountV2(...)`
7. Persist both accounts

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### Parsing

`parse_participate_asset_issue_contract()`:

- Parses protobuf bytes from `transaction.data`
- Extracts:
  - `to_address` (field 2)
  - `asset_name` (field 3)
  - `amount` (field 4)
- **Skips** `owner_address` (field 1) entirely; Rust uses `transaction.from` as “owner”

### Validation & execution

`execute_participate_asset_issue_contract()` performs validation + state updates in one function:

1. Parses contract fields (`to_address`, `asset_name`, `amount`)
2. Validates `to_address` **by length only**:
   - accepts 21 bytes (drops prefix) or 20 bytes (raw)
3. Rejects `owner == to_address`
4. Rejects `amount <= 0`
5. Loads owner account by `transaction.from` (mapped to DB key as `0x41 + 20-byte`)
6. Checks owner balance `>= amount` (fee is 0)
7. Reads `ALLOW_SAME_TOKEN_NAME`
8. Loads `AssetIssueContractData` from either `assetIssue` or `assetIssueV2` store based on `ALLOW_SAME_TOKEN_NAME`, keyed by the raw `asset_name` bytes
9. Validates `to_address` matches `asset_issue.owner_address` (compares 20-byte forms)
10. Validates `now` within `[start_time, end_time)`
11. Computes `exchange_amount = (amount * num) / trx_num` with `checked_mul` and integer division; rejects `<= 0`
12. Loads issuer account (toAddress); errors if missing
13. Checks issuer token balance via `get_asset_balance_v2(to_account, asset_name, allowSameTokenName)`; rejects if `< exchange_amount`
14. Updates:
   - owner balance `- amount`
   - issuer balance `+ amount`
   - owner TRC-10 via `add_asset_amount_v2(...)`
   - issuer TRC-10 via `reduce_asset_amount_v2(...)`
15. Persists both accounts
16. Emits `Trc10Change::AssetTransferred` for parity/reporting:
   - `owner_address = issuer` (sender of tokens)
   - `to_address = participant` (receiver of tokens)
   - `asset_name = contract asset_name bytes`
   - `token_id = Some(asset_issue.id or fallback)`
   - `amount = exchange_amount`

---

## Does it match java-tron?

### What matches (good parity vs `ParticipateAssetIssueActuator`)

- **Core validation logic**: amount positivity, self-participation rejection, owner/to account existence, balance checks, asset existence lookup keyed by `asset_name`, issuer-ownership check, time-window check, exchangeAmount computation and `> 0` requirement, issuer token-balance check.
- **Exchange semantics**: TRX debited from participant and credited to issuer; TRC-10 tokens credited to participant and debited from issuer.
- **ALLOW_SAME_TOKEN_NAME behavior**: store selection for asset issue lookup, and token-balance map selection (`asset` vs `asset_v2`) match the Java `assetBalanceEnoughV2`/`addAssetAmountV2`/`reduceAssetAmountV2` split.
- **Fee**: hardcoded 0 matches `calcFee() = 0` in Java.

### Where it diverges (real mismatches / risk areas)

1) **Owner address validation is skipped (Java validates `owner_address`)**

- Java validates `ParticipateAssetIssueContract.owner_address` via `DecodeUtil.addressValid(...)` and will fail early with `"Invalid ownerAddress"`.
- Rust ignores the field and uses `transaction.from`.

Impact:

- Transactions with malformed `owner_address` bytes can fail with different errors (or later), and Rust cannot reproduce `"Invalid ownerAddress"` from the Java test suite.
- If `transaction.from` (derived from the remote request) is ever inconsistent with the contract’s `owner_address`, Rust behavior can differ from Java’s actuator path.

2) **`to_address` validation is weaker (length-only vs Java’s `DecodeUtil.addressValid`)**

- Java requires `to_address` to be a valid 21-byte TRON address with the correct prefix, else `"Invalid toAddress"`.
- Rust accepts 20 bytes (no TRON prefix) and does not validate prefix/network.

Impact:

- Wrong-prefix addresses (or 20-byte “EVM style” addresses) may pass Rust checks but would be rejected by Java validation.
- Error message parity differs for malformed lengths (Rust: `"Invalid to address length"` vs Java: `"Invalid toAddress"`).

3) **Edge-case stringification differs (`ByteArray.toStr` vs `from_utf8_lossy`)**

Java error messages use `ByteArray.toStr(byte[])`, which returns `null` for empty arrays; Rust uses `String::from_utf8_lossy`, which yields `""` for empty.

Impact:

- In edge cases like `asset_name == []`, Java throws `"No asset named null"` while Rust would throw `"No asset named "` (empty).

4) **Potential incompatibility with TRC-10 “asset optimization” storage**

Java’s TRC-10 helpers call `importAsset(key)` which can load missing `assetV2` entries from the `account-asset` DB when `ALLOW_ASSET_OPTIMIZATION` is enabled.

Rust’s TRC-10 balance helpers (`get_asset_balance_v2`, `add_asset_amount_v2`, `reduce_asset_amount_v2`) operate only on the `Account` proto maps and do not consult `account-asset`.

Impact:

- If `Account.asset_optimized == true` and balances live primarily in `account-asset`, Rust can under-read issuer balances and incorrectly reject with `"Asset balance is not enough !"`, or produce incorrect post-state.

5) **`token_id` empty handling**

Rust errors if the resolved `token_id_str` is empty (`"token_id cannot be empty"`). Java does not explicitly check this in the actuator.

Impact:

- If an asset issue record exists with an empty `id`, Rust rejects while Java may proceed (or fail later in different code paths).

---

## Bottom line

- For normal/mainnet-style transactions (valid TRON addresses, non-empty asset_name, assets with non-empty `id`, and TRC-10 balances present in account protos), Rust’s `PARTICIPATE_ASSET_ISSUE_CONTRACT` implementation is **very close** to the Java actuator logic.
- The main parity gaps are **address validation strictness** (especially `owner_address` and `to_address`) and **missing support for TRC-10 asset optimization (`account-asset` store)**, plus a couple of edge-case message/ID-handling differences.

