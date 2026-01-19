# CLEAR_ABI_CONTRACT (type 48) — Rust backend parity review

## TL;DR
- For the normal remote-execution shape (backend `TronTransaction.contract_parameter` is present), Rust matches java-tron’s **validation ordering**, **error strings**, and the **state transition**: it writes a default/empty ABI message to `AbiStore` keyed by `contract_address`.
- The remaining differences are edge-case/compatibility surfaces: malformed protobuf decoding error text, any-less request heuristics, and simplified bandwidth accounting.

## Java-side oracle (what “correct” means)
Source: `actuator/src/main/java/org/tron/core/actuator/ClearABIContractActuator.java`

### `validate()` checks (order is important)
1) `any == null` → `"No contract!"`
2) `chainBaseManager == null` → `"No account store or contract store!"`
3) `getAllowTvmConstantinople() == 0` → `"contract type error,unexpected type [ClearABIContract]"`
4) `!any.is(ClearABIContract.class)` → `"contract type error,expected type [ClearABIContract],real type[class com.google.protobuf.Any]"`
5) `!DecodeUtil.addressValid(owner_address)` → `"Invalid address"`
6) `AccountStore.get(owner_address) == null` → `"Account[<hex owner>] not exists"`
7) `ContractStore.get(contract_address) == null` → `"Contract not exists"`
8) `owner_address != deployedContract.origin_address` → `"Account[<hex owner>] is not the owner of the contract"`

### `execute()` effect
- Fee: `0`
- Writes empty ABI into `AbiStore`:
  - `abiStore.put(contractAddress, new AbiCapsule(ABI.getDefaultInstance()))`
  - Encoding-wise this is an **empty protobuf message**, so the stored value is `[]` (empty bytes).

## Rust backend implementation (what it actually does)
### Where it lives
- Dispatch: `rust-backend/crates/core/src/service/mod.rs` (`execute_non_vm_contract` → `TronContractType::ClearAbiContract`)
- Contract handler: `rust-backend/crates/core/src/service/mod.rs` (`execute_clear_abi_contract`)
- Parsing: `rust-backend/crates/core/src/service/mod.rs` (`parse_clear_abi_contract`)
- Storage write: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`clear_abi` → `put_abi`)

### Validation + execution flow in Rust
In `execute_clear_abi_contract`:
1) Constantinople gate:
   - Reads dynamic property `ALLOW_TVM_CONSTANTINOPLE` via `storage_adapter.get_allow_tvm_constantinople()`
   - If `0` → returns `"contract type error,unexpected type [ClearABIContract]"`
2) Any type check (mirrors `any.is(...)`) **when `contract_parameter` is present**:
   - If `type_url` is not `protocol.ClearABIContract` (or `.../protocol.ClearABIContract`) → returns
     `"contract type error,expected type [ClearABIContract],real type[class com.google.protobuf.Any]"`
3) Parses `owner_address` and `contract_address` from:
   - `contract_parameter.value` when present, otherwise `transaction.data`
4) Compatibility-only heuristic when `contract_parameter` is missing:
   - If `from_raw` is empty but the payload contains a non-empty owner field, returns the same type-mismatch error as Java would.
5) Owner address validity:
   - Requires `len == 21` and prefix byte matches `storage_adapter.address_prefix()`
   - Else: `"Invalid address"`
6) Owner existence:
   - `get_account_proto(owner)` must exist
   - Else: `"Account[<hex owner>] not exists"`
7) Contract existence + ownership:
   - `get_smart_contract(contract_address)` must exist
   - Else: `"Contract not exists"`
   - `smart_contract.origin_address == owner_address` required
   - Else: `"Account[<hex owner>] is not the owner of the contract"`
8) Execute:
   - `storage_adapter.clear_abi(contract_address)` writes `Abi::default()` to the `abi` DB.
   - `Abi::default()` encodes to empty bytes, matching Java’s stored value.

Return object notes:
- `energy_used = 0` (matches fee-free nature)
- `bandwidth_used` uses a simplified estimator (`calculate_bandwidth_usage`), not exact protobuf serialization size.
- `tron_transaction_result = None` (empty). `framework/src/main/proto/backend.proto` documents that Java should reconstruct the receipt when this is empty.

## Does it match java-tron?
### ✅ Core semantics (state transition)
- Both implementations are effectively:
  - Validate authorization
  - `AbiStore[contract_address] = ABI.getDefaultInstance()`
- Operation is idempotent (“already cleared” still succeeds and leaves empty ABI stored).

### ✅ Validation gates & error strings (normal remote-exec path)
With `contract_parameter` populated (the current fixture + RemoteExecutionSPI path), Rust matches:
- The Constantinople fork gate
- `Any.is(...)`-equivalent type checking (including the same error string)
- Address validity, owner existence, contract existence, and owner==origin checks
- The exact java-tron message wording/formatting used by fixtures

### ⚠️ Edge cases where parity can drift
- Malformed/truncated protobuf bytes:
  - Java throws `InvalidProtocolBufferException` with specific messages.
  - Rust’s `parse_clear_abi_contract` is a lightweight parser and does **not** map errors to the same exception text the way some other parsers in this repo do (e.g., `parse_update_brokerage_contract`).
- Any-less requests:
  - Java always has `Any`; Rust has a best-effort fallback path when `contract_parameter` is absent, which cannot perfectly mirror `Any.is(...)`.
- Bandwidth accounting:
  - Rust uses an approximate formula, so if anything depends on exact net usage, this may diverge.

## Recommendation
- No functional mismatch found for CLEAR_ABI_CONTRACT vs java-tron in the intended path (type_url present + well-formed payload).
- If you want stricter parity for malformed payloads and/or bandwidth/receipt details, use the checklist in `planning/review_again/CLEAR_ABI_CONTRACT.todo.md`.

