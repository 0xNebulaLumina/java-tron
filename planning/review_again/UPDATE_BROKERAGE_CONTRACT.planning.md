# UPDATE_BROKERAGE_CONTRACT (type 49) — Rust backend parity review

## TL;DR
- For the intended remote-execution shape (backend `TronTransaction.metadata.contract_parameter` is present), Rust matches java-tron’s **validation ordering**, **error strings**, and the **state transition**: it writes the brokerage percentage to `DelegationStore` under cycle `-1` (`REMARK`).
- The main parity risks are non-standard/edge inputs: missing `contract_parameter` (`google.protobuf.Any`), malformed protobuf bytes beyond the specific truncation cases mapped today, and any future divergence in address prefix handling.

## Java-side oracle (what “correct” means)
Source: `actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java`

### `validate()` checks (order is important)
1) `any == null` → `"No contract!"`
2) `chainBaseManager == null` → `"No account store or dynamic store!"` (via `ActuatorConstant.STORE_NOT_EXIST`)
3) `!dynamicStore.allowChangeDelegation()` → `"contract type error, unexpected type [UpdateBrokerageContract]"`
4) `!any.is(UpdateBrokerageContract.class)` → `"contract type error, expected type [UpdateBrokerageContract], real type[class com.google.protobuf.Any]"`
5) `any.unpack(UpdateBrokerageContract.class)` failure → throw `InvalidProtocolBufferException.getMessage()`
6) `!DecodeUtil.addressValid(owner_address)` → `"Invalid ownerAddress"`
7) `brokerage < 0 || brokerage > 100` → `"Invalid brokerage"`
8) `witnessStore.get(owner_address) == null` → `"Not existed witness:<hex owner_address>"`
9) `accountStore.get(owner_address) == null` → `"Account does not exist"`

### `execute()` effect
- Fee: `0`
- Writes the brokerage to `DelegationStore`:
  - `delegationStore.setBrokerage(ownerAddress, brokerage)`
  - which is `setBrokerage(-1, ownerAddress, brokerage)`
  - key format (Java): `"{cycle}-{hex(address)}-brokerage"` (see `chainbase/src/main/java/org/tron/core/store/DelegationStore.java`)
  - value encoding (Java): `ByteArray.fromInt(brokerage)` (4-byte big-endian)

## Rust backend implementation (what it actually does)
### Where it lives
- Dispatch: `rust-backend/crates/core/src/service/mod.rs` (`execute_non_vm_contract` → `TronContractType::UpdateBrokerageContract`)
- Contract handler: `rust-backend/crates/core/src/service/mod.rs` (`execute_update_brokerage_contract`)
- Parsing: `rust-backend/crates/core/src/service/mod.rs` (`parse_update_brokerage_contract`)
- Storage write: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`set_delegation_brokerage`)
- Key format: `rust-backend/crates/execution/src/delegation/keys.rs` (`delegation_brokerage_key`)

### Validation + execution flow in Rust
In `execute_update_brokerage_contract`:
1) Delegation feature gate:
   - Reads dynamic property `CHANGE_DELEGATION` via `storage_adapter.allow_change_delegation()`
   - If false → `"contract type error, unexpected type [UpdateBrokerageContract]"`
2) Any type check (mirrors `any.is(...)`) **when `contract_parameter` is present**:
   - If `type_url` is not `protocol.UpdateBrokerageContract` (or `.../protocol.UpdateBrokerageContract`) → returns
     `"contract type error, expected type [UpdateBrokerageContract], real type[class com.google.protobuf.Any]"`
3) Parses `owner_address` (bytes) and `brokerage` (int32) from:
   - `contract_parameter.value` when present, otherwise `transaction.data`
   - Parser is a lightweight protobuf decoder; it maps truncation/EOF cases to protobuf-java’s standard truncation message used by fixtures.
4) Owner address validity:
   - Requires `len == 21` and prefix byte matches `storage_adapter.address_prefix()`
   - Else: `"Invalid ownerAddress"`
5) Brokerage range:
   - `0 <= brokerage <= 100`, else `"Invalid brokerage"`
6) Witness existence:
   - `storage_adapter.is_witness(owner)` must be true
   - Else: `"Not existed witness:<hex owner_address>"`
7) Account existence:
   - `storage_adapter.get_account(owner)` must be `Some`, else `"Account does not exist"`
8) Execute:
   - `storage_adapter.set_delegation_brokerage(-1, owner, brokerage)`
   - key format matches Java: `"{cycle}-{hex(tron_address)}-brokerage"`
   - value encoding is `i32::to_be_bytes()` (4-byte big-endian), matching `ByteArray.fromInt(...)`.

Return object notes:
- `energy_used = 0` and no balance changes (matches the fee-free nature of the Java actuator).
- `bandwidth_used` is computed by Rust’s estimator (`calculate_bandwidth_usage`), not by reproducing Java’s exact net-usage pipeline.

## Does it match java-tron?
### ✅ Core semantics (state transition)
Yes.
- Both implementations write `brokerage` for the witness into DelegationStore under cycle `-1`.
- Key format and value encoding match Java (`-1-<hex 21-byte tron address>-brokerage` → 4-byte big-endian int).

### ✅ Validation ordering + error strings (normal remote-exec path)
With `contract_parameter` present (RemoteExecutionSPI + conformance fixtures), Rust matches Java’s:
- delegation gate (`allowChangeDelegation`)
- `Any.is(...)`-equivalent type_url check (including the exact error string)
- protobuf decode failure message for the truncated/EOF fixture case
- address validity, brokerage bounds, witness existence, account existence checks (and their precedence)

### ⚠️ Edge cases where parity can drift (mostly non-chain-realistic)
- Missing `contract_parameter` (`Any`) in the request:
  - Java always has `Any`; Rust has a fallback path that can’t perfectly mirror `Any.is(...)` semantics.
- Malformed protobuf inputs outside the mapped truncation/EOF patterns:
  - Java may emit different `InvalidProtocolBufferException` messages (e.g., “malformed varint”); Rust currently only normalizes the truncation-style message to match fixtures.
- Inconsistent “from” vs payload owner:
  - Rust has some best-effort behavior for Any-less requests; Java relies on the contract payload’s `owner_address`.

## Recommendation
- No functional mismatch found for `UPDATE_BROKERAGE_CONTRACT` vs java-tron in the intended path (type_url present + well-formed payload).
- If strict parity for pathological protobuf inputs or Any-less requests matters, use the checklist in `planning/review_again/UPDATE_BROKERAGE_CONTRACT.todo.md`.

