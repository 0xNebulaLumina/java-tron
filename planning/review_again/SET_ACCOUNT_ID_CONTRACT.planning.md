# SET_ACCOUNT_ID_CONTRACT (19) — Rust vs Java parity review

## Scope
Review whether the Rust backend implementation of `SET_ACCOUNT_ID_CONTRACT` matches java-tron’s behavior (validation + execution + accountId index persistence).

## References (source of truth)
- Java actuator: `actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java`
- Java validation helpers:
  - `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java` (`validAccountId`)
  - `common/src/main/java/org/tron/common/utils/DecodeUtil.java` (`addressValid`)
- Java index store: `chainbase/src/main/java/org/tron/core/store/AccountIdIndexStore.java`
- Rust executor: `rust-backend/crates/core/src/service/mod.rs` (`execute_set_account_id_contract`, `parse_set_account_id_contract`, `validate_account_id`)
- Rust index storage: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`has_account_id`, `put_account_id_index`, `account_id_key`)
- Java → Rust request mapping: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (sends `SetAccountIdContract.toByteArray()` + raw `Any`)

## What java-tron does (baseline)
`SetAccountIdActuator.validate()` enforces, in order:
1. `Any` exists and is `SetAccountIdContract` (else: `contract type error,expected type [SetAccountIdContract],real type[...]`).
2. `TransactionUtil.validAccountId(accountId)`:
   - length `8..=32`
   - each byte `0x21..=0x7E` (printable ASCII excluding space)
   - else: `Invalid accountId`
3. `DecodeUtil.addressValid(ownerAddress)`:
   - length must be exactly 21 bytes
   - first byte must match the configured address prefix (typically `0x41`)
   - else: `Invalid ownerAddress`
4. Owner account must exist in `AccountStore` (else: `Account has not existed`).
5. Account must not already have an `account_id` (else: `This account id already set`).
6. `AccountIdIndexStore.has(accountId)` must be false (case-insensitive uniqueness via lowercased key) (else: `This id has existed`).

`SetAccountIdActuator.execute()` then:
- Loads the owner `AccountCapsule`, sets `account_id` to the provided bytes, persists it back to `AccountStore`.
- Writes `AccountIdIndexStore.put(account)`, which stores:
  - key = `lowercase(UTF-8(account_id))` (via `String.toLowerCase()`),
  - value = `ownerAddress` (21-byte address).
- Fee is `0`.

## What Rust does today
`execute_set_account_id_contract` in `rust-backend/crates/core/src/service/mod.rs`:
- Verifies `Any.type_url` tail matches `protocol.SetAccountIdContract` (when `metadata.contract_parameter` is present), returning the same java-tron error string on mismatch.
- Uses `transaction.from` (20-byte EVM address) as the “owner” key, and uses `transaction.metadata.from_raw` for address validation.
- Parses the contract bytes with a lightweight protobuf scan (`parse_set_account_id_contract`), extracting only `account_id` (field 1) and ignoring `owner_address` (field 2).
- Validates `account_id` with the same byte-range + length rules (`validate_account_id`), returning `Invalid accountId` on failure.
- Validates owner address bytes in `from_raw` with:
  - length 21 => prefix must match `storage_adapter.address_prefix()`
  - length 20 => accepted as valid
  - else => `Invalid ownerAddress`
- Loads the owner account (`get_account_proto`); if absent => `Account has not existed`.
- Enforces immutability (`account_id` must be empty) and uniqueness (`has_account_id`), returning the same java-tron strings.
- Persists the updated account, then persists the account-id index entry:
  - key normalization in Rust storage: `account_id_key()` lowercases *UTF-8* bytes using Rust’s `to_lowercase()`
  - value stored: `to_tron_address_21(owner)` (21-byte address with the DB’s prefix)

## Where it may NOT match java-tron (important edge cases)

### 1) Contract `owner_address` is not parsed/validated against `from`
Java uses `setAccountIdContract.getOwnerAddress()` as the source of truth for:
- address validation, and
- selecting which account is mutated.

Rust validates `transaction.metadata.from_raw` and mutates `transaction.from`, but does **not** parse and validate the `owner_address` field inside the `SetAccountIdContract` bytes.

In the normal java-tron → Rust pipeline, this is *probably OK* because `RemoteExecutionSPI` uses `trxCap.getOwnerAddress()` as `tx.from`, and it serializes the same contract bytes.

But strictly speaking:
- if `tx.from` and `contract.owner_address` ever diverge (malformed request, bug, or non-java caller), Rust could update a different account than java-tron would.

### 2) Owner address validation is slightly looser (20-byte accepted)
`DecodeUtil.addressValid` in Java requires exactly 21 bytes and correct prefix.

Rust’s validation path accepts 20-byte `from_raw` as valid. If a caller sends 20 bytes, Rust may accept a transaction that java-tron would reject with `Invalid ownerAddress`.

### 3) Lowercasing semantics can differ in rare locale cases (case-insensitive uniqueness)
Java’s `AccountIdIndexStore` lowercases via `String.toLowerCase()` (default locale).
Rust lowercases via `to_lowercase()` (Unicode case mapping, locale-insensitive).

Given `validAccountId` allows uppercase ASCII, a node running java-tron under a Turkish locale could theoretically lowercase `I` differently (`I -> ı`) than Rust (`I -> i`), producing different index keys and (worse) different uniqueness behavior.

This is arguably a java-tron determinism footgun, but it’s still a potential Rust-vs-Java mismatch if locales differ.

### 4) Malformed protobuf bytes: parser behavior may diverge
Java uses protobuf unpacking and will throw `InvalidProtocolBufferException` on malformed encodings.
Rust uses a minimal field scan that can “skip” inconsistent lengths and default `account_id` to empty, typically surfacing as `Invalid accountId` rather than a protobuf decoding error string.

Not an issue for normal requests (java-tron serializes valid bytes), but relevant if you care about strict parity under adversarial/malformed inputs.

## Bottom line
- For the intended java-tron remote execution pipeline and “normal” inputs, **Rust’s SET_ACCOUNT_ID_CONTRACT implementation is functionally aligned with java-tron**: same validation rules, same state changes, same uniqueness model (case-insensitive via lowercased index key), fee/energy `0`.
- If you need *strict* behavioral equivalence across unusual inputs (or want stronger robustness), the main gaps are:
  - not parsing/cross-checking `owner_address` inside the contract bytes,
  - accepting 20-byte owner bytes as valid,
  - potential locale-driven lowercasing differences.

