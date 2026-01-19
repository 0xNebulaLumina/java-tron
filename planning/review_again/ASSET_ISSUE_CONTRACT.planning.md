# Review: `ASSET_ISSUE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_asset_issue_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `AssetIssueActuator` in `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java`

It focuses on whether the Rust implementation matches **java-tron’s actuator semantics** for TRC-10 issuance (validation + state changes), and calls out surrounding behavior that can affect end-to-end parity (domain journal / CSV reporting).

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`AssetIssueActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java`

Key checks (in order):

1. `any.is(AssetIssueContract.class)` → contract type error message on mismatch
2. `DecodeUtil.addressValid(ownerAddress)` → **requires 21 bytes** and **prefix == DecodeUtil.addressPreFixByte**
3. `TransactionUtil.validAssetName(name)` and (if `ALLOW_SAME_TOKEN_NAME != 0`) `name.toLowerCase() != "trx"`
4. `precision` check (only in same-token-name mode): `precision == 0 || (0..=6)`
5. `abbr` validity if non-empty
6. `url` validity
7. `description` validity
8. `start_time != 0`, `end_time != 0`, `end_time > start_time`
9. `start_time > latestBlockHeaderTimestamp`
10. If `ALLOW_SAME_TOKEN_NAME == 0`: reject if `AssetIssueStore.get(nameBytes) != null` (“Token exists”)
11. `total_supply > 0`, `trx_num > 0`, `num > 0`
12. `public_free_asset_net_usage == 0`
13. `frozen_supply_count <= MAX_FROZEN_SUPPLY_NUMBER`
14. `free_asset_net_limit` and `public_free_asset_net_limit` in `[0, ONE_DAY_NET_LIMIT)`
15. Frozen supply schedule:
    - each `frozen_amount > 0`
    - sum of frozen amounts <= total supply
    - each `frozen_days` within `[MIN_FROZEN_SUPPLY_TIME, MAX_FROZEN_SUPPLY_TIME]`
16. Owner account must exist and must not have already issued an asset
17. Owner balance must be `>= AssetIssueFee`

### 2) Execution (`AssetIssueActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java`

1. `fee = DynamicPropertiesStore.getAssetIssueFee()`
2. Allocate token id:
   - `tokenIdNum = getTokenIdNum(); tokenIdNum++; saveTokenIdNum(tokenIdNum)`
   - set `contract.id = String.valueOf(tokenIdNum)` (both V1 and V2 capsules)
3. Persist asset metadata:
   - If `ALLOW_SAME_TOKEN_NAME == 0`:
     - write V1 `AssetIssueStore` keyed by `nameBytes`
     - write V2 `AssetIssueV2Store` keyed by `idBytes`, with **precision forced to 0**
   - Else:
     - write only V2 store keyed by idBytes (precision as provided)
4. Deduct fee from owner balance
5. Fee destination:
   - if `supportBlackHoleOptimization()`: `burnTrx(fee)` (increments `BURN_TRX_AMOUNT`)
   - else: credit blackhole account by `fee`
6. Update owner account:
   - compute `remainSupply = totalSupply - sum(frozen_amount)`
   - if `ALLOW_SAME_TOKEN_NAME == 0`: `account.asset[nameKey] = remainSupply`
   - `assetIssuedName = nameBytes`, `assetIssuedID = idBytes`, `assetV2[idStr] = remainSupply`
   - append `Account.frozen_supply` entries with `expireTime = startTime + frozenDays * FROZEN_PERIOD`
7. Receipt:
   - `ret.setAssetIssueID(String.valueOf(tokenIdNum))`
   - `ret.setStatus(fee, SUCESS)`
8. Optional domain journal:
   - If `DomainChangeRecorderContext.isEnabled()`, emits “create” deltas for issuance fields

---

## Rust implementation behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### Validation & parsing

`execute_asset_issue_contract()`:

- Checks `Any.type_url` matches `protocol.AssetIssueContract` when `transaction.metadata.contract_parameter` is present.
- Decodes `protocol::AssetIssueContractData` via prost (used for `owner_address` and `frozen_supply` validation).
- Parses a minimal `AssetIssueInfo` from protobuf bytes using `parse_asset_issue_contract()`:
  - name/abbr/description/url are decoded as UTF-8 strings using `String::from_utf8_lossy`.
  - numeric fields are parsed as varints.
- Runs validations intended to match `AssetIssueActuator.validate` ordering and message strings.
- Gates execution on `execution_config.remote.trc10_enabled` (if disabled, returns an error to force Java fallback).

### Persistence / state updates

On success, Rust:

1. Reads `ALLOW_SAME_TOKEN_NAME`
2. Reads fee (`ASSET_ISSUE_FEE`) and checks owner balance
3. Allocates token id:
   - reads `TOKEN_ID_NUM`, increments, persists the new value, and sets `asset_proto.id = token_id_str`
4. Persists asset metadata:
   - `ALLOW_SAME_TOKEN_NAME == 0`:
     - writes AssetIssue (V1) by `nameBytes`
     - writes AssetIssueV2 (V2) by `token_id_str.as_bytes()` with **precision forced to 0**
   - else writes only V2 by token id
5. Updates owner account proto:
   - subtracts the fee from `Account.balance`
   - appends `Account.frozen_supply` entries derived from `FrozenSupply`
   - sets `asset_issued_name`, `asset_issued_id`, and inserts into `asset` (legacy) / `asset_v2` (always)
6. Fee destination:
   - if `support_black_hole_optimization()`: increments `BURN_TRX_AMOUNT` (`burn_trx`)
   - else credits blackhole account balance
7. Builds a `TronExecutionResult` that includes:
   - `tron_transaction_result` receipt bytes containing `fee` and `assetIssueID`
   - `trc10_changes` with `Trc10Change::AssetIssued` (for remote CSV/domain reporting)
   - `bandwidth_used` + optional tracked AEXT updates

---

## Does it match java-tron?

### What matches (good parity vs `AssetIssueActuator`)

- **Validation coverage & ordering**: Rust mirrors the Java checks for name/abbr/url/description/time windows, frozen supply bounds, token uniqueness (legacy mode), “one asset per account”, and “enough balance for fee”.
- **Fee source**: uses `ASSET_ISSUE_FEE`.
- **Token id allocation**: `TOKEN_ID_NUM` is incremented before use and persisted.
- **Store writes**:
  - Legacy mode writes both V1 (name-keyed) and V2 (id-keyed) assets.
  - V2 precision forced to `0` when `ALLOW_SAME_TOKEN_NAME == 0`, matching `AssetIssueActuator`.
- **Account updates**:
  - Deducts fee from balance.
  - Sets `asset_issued_name` and `asset_issued_id`.
  - Adds the issued supply into the correct TRC-10 balance map(s).
  - Appends `frozen_supply` entries with the same expiration calculation (`FROZEN_PERIOD = 86_400_000ms`).
- **Blackhole optimization**: burn vs credit blackhole matches Java’s `supportBlackHoleOptimization()` behavior.
- **Receipt parity**: includes `fee` + `assetIssueID` in `tron_transaction_result` (Java sets both fields on `TransactionResultCapsule`).

### Where it diverges (real mismatches / risk areas)

1) **Address prefix strictness (Java is stricter)**

- Java `DecodeUtil.addressValid` requires prefix == `DecodeUtil.addressPreFixByte` (network-specific).
- Rust accepts `0x41` *or* `0xa0` for owner address validation in this contract, and gRPC conversion helpers currently add `0x41` unconditionally when re-attaching prefixes for emitted changes.

Impact:

- Wrong-prefix addresses may pass Rust validation and fail later with a different error (e.g., “Account not exists”), whereas Java would fail early with “Invalid ownerAddress”.
- On non-mainnet DBs (prefix `0xa0`), emitted TRC-10 change owner addresses can be wrong if `0x41` is always used.

2) **Missing dynamic-property behavior differs**

Several storage adapter getters default when keys are absent (e.g., `ALLOW_SAME_TOKEN_NAME` defaults to `0`, `TOKEN_ID_NUM` defaults to `1_000_000`), while Java often throws if a key is missing from `DynamicPropertiesStore`.

Impact:

- Rust can “succeed” (or take legacy paths) in partially initialized DBs where Java would error.
- This can affect token id allocation and mode selection in edge fixtures.

3) **Lossy UTF-8 decoding is used for validation inputs**

Rust parses name/abbr/description/url as strings via `from_utf8_lossy` and then validates on the resulting `.as_bytes()`. Java validates raw bytes.

Impact:

- For malformed UTF-8 (or unusual byte sequences), Rust may validate different bytes than Java, potentially changing which validation fails first and/or token lookup keys in legacy mode.

4) **`Trc10Change::AssetIssued.token_id` is left empty**

Rust already computes `token_id_str`, but it emits `token_id: None` in the TRC-10 change and relies on Java-side logic (`ExecutionCsvRecordBuilder`) to read `TOKEN_ID_NUM` from `DynamicPropertiesStore`.

Impact:

- Works if Java reads the same underlying dynamic store state *after* Rust persisted the increment.
- Can break reporting parity if execution is ever used in an “executor-only” mode where dynamic properties are not shared.

5) **Bytes source split (`contract_parameter` vs `transaction.data`)**

Rust decodes the full proto from `contract_parameter.value` (when present), but parses `AssetIssueInfo` from `transaction.data`. In the current Java remote mapping these are the same bytes, but the Rust code assumes that invariant.

Impact:

- Alternative callers that populate only `contract_parameter` (or only `data`) could diverge in parsing/validation vs persistence.

---

## Bottom line

- For the common/mainnet path (prefix `0x41`, dynamic properties present, UTF-8 token fields), the Rust implementation is **very close** to java-tron’s `AssetIssueActuator.validate + execute` behavior and produces equivalent persisted stores.
- The main parity risks are around **network prefix strictness**, **dynamic-property missing-key defaults**, and **string/byte handling** for invalid inputs, plus a **brittle reporting dependency** (`token_id` omitted from `Trc10Change::AssetIssued`).

