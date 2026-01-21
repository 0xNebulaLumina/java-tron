# UPDATE_ENERGY_LIMIT_CONTRACT (type 45) — Rust backend parity review

## TL;DR
- **State transition** matches: both sides update `SmartContract.origin_energy_limit` for an existing contract.
- **Validation parity does not match** java-tron today when the EnergyLimit fork gate is enabled: Rust ignores the payload’s `owner_address` field (so it misses `DecodeUtil.addressValid` behavior) and emits different error strings for common failure cases.
- The EnergyLimit fork gate itself is **hard-coded** in Rust (`4727890`) instead of using java-tron’s configurable `CommonParameter.blockNumForEnergyLimit`, so it diverges on tests/private nets and makes it hard to generate “fork enabled” fixtures at low block numbers.

## Java-side oracle (what “correct” means)
Sources:
- `actuator/src/main/java/org/tron/core/actuator/UpdateEnergyLimitContractActuator.java`
- `chainbase/src/main/java/org/tron/core/capsule/ReceiptCapsule.java` (`checkForEnergyLimit`)
- Unit tests: `framework/src/test/java/org/tron/core/actuator/UpdateEnergyLimitContractActuatorTest.java`

### Fork gate (must pass before any other validation)
- `ReceiptCapsule.checkForEnergyLimit(ds)` returns:
  - `ds.getLatestBlockHeaderNumber() >= CommonParameter.getInstance().getBlockNumForEnergyLimit()`

### `validate()` checks (order is important)
In `UpdateEnergyLimitContractActuator.validate()`:
1) `any == null` → `"No contract!"`
2) `chainBaseManager == null` → `"No account store or dynamic store!"`
3) `!checkForEnergyLimit(dynamicPropertiesStore)` → `"contract type error, unexpected type [UpdateEnergyLimitContract]"`
4) `!any.is(UpdateEnergyLimitContract.class)` → `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`
5) `any.unpack(UpdateEnergyLimitContract.class)` failure → `InvalidProtocolBufferException.getMessage()`
6) `!DecodeUtil.addressValid(owner_address)` → `"Invalid address"`
7) `AccountStore.get(owner_address) == null` → `"Account[<hex owner>] does not exist"`
8) `origin_energy_limit <= 0` → `"origin energy limit must be > 0"`
9) `ContractStore.get(contract_address) == null` → `"Contract does not exist"`
10) `owner_address != deployedContract.origin_address` → `"Account[<hex owner>] is not the owner of the contract"`

### `execute()` effect
- Fee: `0`
- Writes updated `originEnergyLimit` into `ContractStore` for `contract_address`.
- Also calls `RepositoryImpl.removeLruCache(contractAddress)` (cache-only side effect).

## Rust backend implementation (what it actually does)
### Where it lives
- Dispatch: `rust-backend/crates/core/src/service/mod.rs` (`execute_non_vm_contract` → `TronContractType::UpdateEnergyLimitContract`)
- Contract handler: `rust-backend/crates/core/src/service/mod.rs` (`execute_update_energy_limit_contract`)
- Parsing: `rust-backend/crates/core/src/service/mod.rs` (`parse_update_energy_limit_contract`)
- Fork gate: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`check_for_energy_limit`)

### Validation + execution flow in Rust today
In `execute_update_energy_limit_contract`:
1) EnergyLimit gate:
   - Calls `storage_adapter.check_for_energy_limit()`.
   - If false → `"contract type error, unexpected type [UpdateEnergyLimitContract]"`
2) Any type_url check (mirrors `any.is(...)`) **when `contract_parameter` is present**:
   - If `type_url` is not `protocol.UpdateEnergyLimitContract` (or `.../protocol.UpdateEnergyLimitContract`) →
     `"contract type error, expected type [UpdateEnergyLimitContract],real type[class com.google.protobuf.Any]"`
3) Parses `contract_address` + `origin_energy_limit` from `contract_parameter.value` (or `transaction.data`).
   - NOTE: the parser **skips** `owner_address` and never returns it to the validator.
   - If `contract_address` is empty → returns `"contract_address is required"`.
4) Uses `transaction.from` (not the payload’s owner field) to:
   - check owner account existence (error text: `"Owner account <base58> does not exist"`)
   - compute the `owner_key` used for the origin_address ownership check.
5) Checks `origin_energy_limit > 0` (matches java message).
6) Loads contract from ContractStore and checks `origin_address == owner_key`.
7) Writes `origin_energy_limit` back to ContractStore.

### Fork gate mismatch detail
In `EngineBackedEvmStateStore.check_for_energy_limit()`:
- `threshold` is currently hard-coded via `get_block_num_for_energy_limit()` to `4727890`.
- Java uses `CommonParameter.getBlockNumForEnergyLimit()` which is configurable (tests often set it to `0`).

## Does it match java-tron?
### ✅ Core semantics (state transition)
Yes, when the handler executes successfully: Rust updates the stored smart contract’s `origin_energy_limit`, same as Java.

### ❌ Validation ordering + error strings (fork enabled path)
Not yet. The main parity gaps:

1) **Owner address handling is not Java-parity**
   - Java validates `owner_address` from the payload with `DecodeUtil.addressValid(...)` and errors `"Invalid address"` first.
   - Rust ignores payload `owner_address` entirely and instead trusts `transaction.from` (which can be `Address::ZERO` when the incoming `from` bytes are malformed).
   - Result: malformed/empty/wrong-length owner bytes will typically surface as `"Owner account <base58> does not exist"` in Rust, where Java returns `"Invalid address"`.

2) **Account-not-exist error message differs**
   - Java: `"Account[<hex owner>] does not exist"` (hex string via `StringUtil.createReadableString`).
   - Rust: `"Owner account <base58> does not exist"` (uses `tron_backend_common::to_tron_address` which encodes Base58Check).

3) **Empty contract_address error differs**
   - Java does not have an explicit “missing contract address” check; `ContractStore.get(ByteString.EMPTY)` just yields null → `"Contract does not exist"`.
   - Rust currently fails earlier with `"contract_address is required"`.

4) **Fork gate configurability differs**
   - Java’s `blockNumForEnergyLimit` is a runtime config knob; unit tests (and fixture generators) often set it explicitly.
   - Rust hard-codes the threshold, so parity breaks on non-mainnet configurations and makes “fork enabled” conformance coverage difficult unless the dynamic store’s latest block number is set above `4727890`.

## Recommendation
- Treat `UPDATE_ENERGY_LIMIT_CONTRACT` as **not parity-complete** in Rust until the owner/contract validation rules and error strings are aligned with java-tron and the fork threshold is configurable.
- If you want to enable it (Rust flag `execution.remote.update_energy_limit_enabled` + Java flag `-Dremote.exec.contract.enabled=true`) or expand fixtures to cover fork-enabled branches, follow `planning/review_again/UPDATE_ENERGY_LIMIT_CONTRACT.todo.md`.

