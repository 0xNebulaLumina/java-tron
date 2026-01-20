# Review: `WITHDRAW_EXPIRE_UNFREEZE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_withdraw_expire_unfreeze_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `WithdrawExpireUnfreezeActuator` in `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`
- **gRPC conversion path (important for parity)**:
  - `convert_protobuf_transaction()` in `rust-backend/crates/core/src/service/grpc/conversion.rs`
  - `strip_tron_address_prefix()` in `rust-backend/crates/core/src/service/grpc/address.rs`
- **Conformance fixtures**: `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`

Goal: determine whether Rust matches **java-tron actuator semantics** (validation + state transitions + receipt behavior), including cases where fixtures intentionally include malformed addresses.

---

## Java-side reference behavior (what “correct” means)

Source: `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`

### 1) Validation (`validate`)

Key checks (in order):

1. `any.is(WithdrawExpireUnfreezeContract.class)` (type check)
2. `dynamicStore.supportUnfreezeDelay()` gate (requires `UNFREEZE_DELAY_DAYS > 0`)
3. Decode contract and read `owner_address`
4. `DecodeUtil.addressValid(ownerAddress)` (non-empty, 21 bytes, correct prefix)
5. Owner account exists (`Account[<hex>] not exists`)
6. `totalWithdrawUnfreeze = sum(unfreeze_amount for entries where unfreeze_expire_time <= now)`
   - if `totalWithdrawUnfreeze <= 0`: `"no unFreeze balance to withdraw "` (note trailing space)
7. Overflow check: `LongMath.checkedAdd(balance, totalWithdrawUnfreeze)` (throws `ArithmeticException` with message `overflow: checkedAdd(a, b)`)

### 2) Execution (`execute`)

High-level behavior:

- `fee = 0`
- `now = latestBlockHeaderTimestamp`
- Recompute `totalWithdrawUnfreeze` (same filter `<= now`)
- `balance += totalWithdrawUnfreeze`
- `unfrozenV2 = only entries with unfreeze_expire_time > now` (expired entries removed)
- Receipt: `withdraw_expire_amount = totalWithdrawUnfreeze`

Note: java-tron also has a TVM-native processor (`actuator/.../WithdrawExpireUnfreezeProcessor.java`) with slightly different validation (`< 0` vs `<= 0`), but the **transaction contract** parity target is the actuator above.

---

## Rust backend behavior (current)

Source: `rust-backend/crates/core/src/service/mod.rs` (`execute_withdraw_expire_unfreeze_contract`)

### Validation & gating

Rust performs the same actuator-level checks:

- Requires `transaction.metadata.contract_parameter` and verifies `type_url` matches `protocol.WithdrawExpireUnfreezeContract` (mirrors `any.is(...)`)
- Gate check: `storage_adapter.support_unfreeze_delay()` (mirrors `supportUnfreezeDelay()`)
- Parses `owner_address` from `contract_parameter.value` (field 1)
- Validates owner address as **21 bytes + correct prefix**
- Requires owner account exists (`Account[<hex>] not exists`)
- Computes `total_withdraw` as the sum of expired `unfrozen_v2` entries (`unfreeze_expire_time <= now`)
  - Uses `wrapping_add` for the sum, matching Java `long` overflow semantics in `LongStream.sum()`
- Requires `total_withdraw > 0` with the same error string: `"no unFreeze balance to withdraw "`
- Checks overflow when updating balance via `checked_add`, returning an error string matching Guava:
  - `overflow: checkedAdd(balance, total_withdraw)`

### State transitions

On success:

- `account.balance += total_withdraw`
- Removes expired entries from `unfrozen_v2` and preserves unexpired entries (order preserved)
- Persists the updated account
- Receipt: builds `Transaction.Result` bytes with `withdraw_expire_amount = total_withdraw`

---

## Parity assessment

### ✅ Core actuator semantics: match (for well-formed requests)

For requests where the transaction can be decoded (i.e., the protobuf `TronTransaction.from` field is valid enough for Rust conversion to succeed), Rust matches java-tron on:

- Type check + gate check + error messages
- Address validation and account-not-exists error formatting
- Expired boundary: `expire_time == now` is treated as expired (`<= now`)
- Withdrawal amount computation, including `long` overflow semantics
- Balance overflow detection message shape (`overflow: checkedAdd(a, b)`)
- Receipt field (`withdraw_expire_amount`) on success

### ⚠️ End-to-end parity gap: conversion rejects malformed/empty `from` before contract validation

java-tron validates the owner address from the decoded contract bytes. For an empty owner address it fails with `"Invalid address"`.

In the Rust backend, `convert_protobuf_transaction()` currently tries to parse `tx.from` up-front via `strip_tron_address_prefix()`, which rejects any length other than 20 or 21 bytes. The “allow malformed from” exception list in `rust-backend/crates/core/src/service/grpc/conversion.rs` does **not** include `WithdrawExpireUnfreezeContract`.

This matters for the conformance fixture:

- `generateWithdrawExpireUnfreeze_ownerAddressInvalidEmpty()` builds a `WithdrawExpireUnfreezeContract` with `owner_address = ByteString.EMPTY`.
- `RemoteExecutionSPI` maps system contracts by setting `TronTransaction.from` from that owner address.
- Rust conversion will fail early (address-length error) instead of reaching `execute_withdraw_expire_unfreeze_contract()` and returning `"Invalid address"`.

So: the contract execution logic itself matches Java, but the current conversion gate prevents Java-equivalent validation for malformed-owner fixtures.

---

## Conclusion

- **Actuator-level logic parity (contract execution)**: ✅ looks aligned with java-tron’s `WithdrawExpireUnfreezeActuator`.
- **End-to-end parity in the remote pipeline**: ⚠️ likely fails for malformed/empty owner-address fixtures due to `tx.from` conversion happening before contract-level validation.

