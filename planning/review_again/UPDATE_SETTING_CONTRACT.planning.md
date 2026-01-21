# UPDATE_SETTING_CONTRACT (type 33) — Rust backend parity review

## TL;DR
- For the normal remote-execution shape (backend `TronTransaction.metadata.contract_parameter` is present), Rust matches java-tron’s **validation ordering**, **error strings**, and the **state transition**: it updates `SmartContract.consume_user_resource_percent` in `ContractStore` for an existing contract owned by the caller.
- The remaining differences are edge-case/compatibility surfaces: malformed protobuf decoding error text, and Any-less request heuristics (which are not used by `RemoteExecutionSPI` / current fixtures).

## Java-side oracle (what “correct” means)
Sources:
- `actuator/src/main/java/org/tron/core/actuator/UpdateSettingContractActuator.java`
- Unit tests: `framework/src/test/java/org/tron/core/actuator/UpdateSettingContractActuatorTest.java`

### `validate()` checks (order is important)
In `UpdateSettingContractActuator.validate()`:
1) `any == null` → `"No contract!"`
2) `chainBaseManager == null` → `"No account store or contract store!"`
3) `!any.is(UpdateSettingContract.class)` → `"contract type error, expected type [UpdateSettingContract], real type[class com.google.protobuf.Any]"`
4) `any.unpack(UpdateSettingContract.class)` failure → `InvalidProtocolBufferException.getMessage()`
5) `!DecodeUtil.addressValid(owner_address)` → `"Invalid address"`
6) `AccountStore.get(owner_address) == null` → `"Account[<hex owner>] does not exist"`
7) `consume_user_resource_percent` not in `[0, 100]` → `"percent not in [0, 100]"`
8) `ContractStore.get(contract_address) == null` → `"Contract does not exist"`
9) `owner_address != deployedContract.origin_address` → `"Account[<hex owner>] is not the owner of the contract"`

### `execute()` effect
- Fee: `0`
- Writes updated `consumeUserResourcePercent` into `ContractStore` for `contract_address`.
- Also calls `RepositoryImpl.removeLruCache(contractAddress)` (cache-only side effect).

## Rust backend implementation (what it actually does)
### Where it lives
- Dispatch: `rust-backend/crates/core/src/service/mod.rs` (`execute_non_vm_contract` → `TronContractType::UpdateSettingContract`)
- Contract handler: `rust-backend/crates/core/src/service/mod.rs` (`execute_update_setting_contract`)
- Parsing: `rust-backend/crates/core/src/service/mod.rs` (`parse_update_setting_contract`)
- Storage read/write: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`get_smart_contract` / `put_smart_contract`)

### Validation + execution flow in Rust
In `execute_update_setting_contract`:
1) Any type check (mirrors `any.is(...)`) **when `contract_parameter` is present**:
   - If `type_url` is not `protocol.UpdateSettingContract` (or `.../protocol.UpdateSettingContract`) → returns
     `"contract type error, expected type [UpdateSettingContract], real type[class com.google.protobuf.Any]"`
2) Parses `(owner_address, contract_address, consume_user_resource_percent)` from:
   - `contract_parameter.value` when present, otherwise `transaction.data`
3) Compatibility-only heuristic when `contract_parameter` is missing:
   - If `from_raw` is empty but the payload contains a non-empty owner field, returns the same type-mismatch error as Java would.
4) Owner address validity:
   - Requires `len == 21` and prefix byte matches `storage_adapter.address_prefix()`
   - Else: `"Invalid address"`
5) Owner existence:
   - `get_account(owner)` must exist
   - Else: `"Account[<hex owner>] does not exist"`
6) Percent range:
   - Rejects `< 0` or `> 100` → `"percent not in [0, 100]"`
7) Contract existence + ownership:
   - `get_smart_contract(contract_address)` must exist
   - Else: `"Contract does not exist"`
   - `smart_contract.origin_address == owner_address` required
   - Else: `"Account[<hex owner>] is not the owner of the contract"`
8) Execute:
   - Updates `smart_contract.consume_user_resource_percent` and persists via `put_smart_contract`.

Return object notes:
- `energy_used = 0` and no balance/state changes (matches fee-free nature).
- `bandwidth_used` uses a simplified estimator (`calculate_bandwidth_usage`), not exact protobuf serialization size.

## Does it match java-tron?
### ✅ Core semantics (state transition)
- Both implementations update the stored smart contract’s `consume_user_resource_percent`.

### ✅ Validation ordering + error strings (normal remote-exec path)
With `contract_parameter` populated (the `RemoteExecutionSPI` + current conformance fixtures path), Rust matches:
- `Any.is(...)`-equivalent type checking (including the exact fixture string)
- Address validity, owner existence, percent range, contract existence, and owner==origin checks
- The exact java-tron message wording/formatting used by fixtures

### ⚠️ Edge cases where parity can drift
- Malformed/truncated protobuf bytes:
  - Java throws `InvalidProtocolBufferException` with its own messages.
  - Rust uses a lightweight parser and returns Rust-specific parse errors.
- Any-less requests:
  - Java always has `Any`; Rust has a best-effort fallback when `contract_parameter` is absent, which cannot perfectly mirror `Any.is(...)`.
  - Rust also falls back to `from_raw` when the payload omits `owner_address`; Java validates only the payload owner field.

## Recommendation
- No functional mismatch found for UPDATE_SETTING_CONTRACT vs java-tron in the intended path (type_url present + well-formed payload).
- If you want stricter parity for malformed payloads and/or to remove Any-less heuristics, use `planning/review_again/UPDATE_SETTING_CONTRACT.todo.md`.

