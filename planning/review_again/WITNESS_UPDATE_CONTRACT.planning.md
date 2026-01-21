# Review: `WITNESS_UPDATE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_witness_update_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `WitnessUpdateActuator` in `actuator/src/main/java/org/tron/core/actuator/WitnessUpdateActuator.java`
- **Java URL validation**: `TransactionUtil.validUrl` in `actuator/src/main/java/org/tron/core/utils/TransactionUtil.java`
- **Witness storage**:
  - Java: `WitnessCapsule` in `chainbase/src/main/java/org/tron/core/capsule/WitnessCapsule.java`
  - Rust: `WitnessInfo` encoding in `rust-backend/crates/execution/src/storage_adapter/types.rs` and persistence in `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- **Java→Rust request mapping**: `RemoteExecutionSPI` in `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (how `tx.from` / `tx.data` are populated)

Goal: determine whether Rust remote execution matches java-tron’s **validation + state transition** for witness URL updates, and identify any state-/consensus-relevant mismatches.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`WitnessUpdateActuator.validate`)

Key checks (and messages) in order:

1. Contract type: `any.is(WitnessUpdateContract.class)`
   - Error prefix: `contract type error, expected type [WitnessUpdateContract],real type[...]`
2. `DecodeUtil.addressValid(ownerAddress)` (requires 21 bytes and correct prefix)
   - Error: `Invalid address`
3. Owner account exists (`accountStore.has(ownerAddress)`)
   - Error: `account does not exist`
4. URL bytes valid (`TransactionUtil.validUrl(updateUrlBytes)`):
   - non-empty, ≤ 256 bytes (no URI/ASCII validation)
   - Error: `Invalid url`
5. Witness exists (`witnessStore.has(ownerAddress)`)
   - Error: `Witness does not exist`

### 2) Execution (`WitnessUpdateActuator.execute` + `updateWitness`)

State transition:

- Load `WitnessCapsule` for `ownerAddress`
- Update **only** the URL: `witnessCapsule.setUrl(update_url.toStringUtf8())`
- Persist: `witnessStore.put(witnessCapsule.createDbKey(), witnessCapsule)`
- Fee: `0` (`calcFee()` returns 0)

Important: only `url` changes; all other witness fields remain untouched.

---

## Rust implementation behavior (what it currently does)

`execute_witness_update_contract()`:

- Validates owner address using `transaction.metadata.from_raw` (requires 21 bytes + `storage_adapter.address_prefix()`).
- Requires owner account exists (`storage_adapter.get_account(&transaction.from)`).
- Interprets `transaction.data` as `update_url` bytes; validates non-empty and ≤ 256 bytes; decodes via `String::from_utf8_lossy(...)`.
- Requires witness exists (`storage_adapter.get_witness(&owner)`).
- Writes witness via `storage_adapter.put_witness(&WitnessInfo{ address, url=new_url, vote_count=existing.vote_count, ... })`
  - currently skips the write if `new_url == old_url`.
- Returns `TronExecutionResult { success=true, energy_used=0, logs=[], state_changes=[] }` (+ bandwidth / AEXT accounting).

Java→Rust request mapping:

- `tx.from = owner_address` bytes (TRON-style address)
- `tx.data = WitnessUpdateContract.update_url` bytes (URL payload only, not the full proto)
- `tx.contract_parameter = Transaction.Contract.parameter` (raw Any: `type_url` + `value`)

This mapping matches Rust’s assumption that `transaction.data` is the URL payload.

---

## Does it match java-tron?

### What matches (good parity)

- **Validation order + error strings** for:
  - `Invalid address`
  - `account does not exist`
  - `Invalid url`
  - `Witness does not exist`
- **URL validation semantics**: only checks emptiness and max length (≤ 256 bytes), same as Java `TransactionUtil.validUrl`.
- **UTF-8 decode behavior**: Rust `from_utf8_lossy` and Java `ByteString.toStringUtf8()` both replace invalid sequences.
- **Fee/energy/logs**: no fee, `energy_used=0`, no logs.

### Where it diverges (real mismatch)

1) **Witness record field clobbering (state mismatch)**

Java updates only `Witness.url` and preserves all other witness fields.

Rust currently round-trips witness state through `tron_backend_execution::WitnessInfo`, which is a *lossy projection* of the Java `protocol.Witness` proto:

- `WitnessInfo` only carries `{ address, url, vote_count }` (`rust-backend/crates/execution/src/storage_adapter/types.rs`).
- `WitnessInfo::deserialize()` ignores other `protocol.Witness` fields (e.g. `pub_key`, `total_produced`, `total_missed`, `latest_block_num`, `latest_slot_num`, `is_jobs`).
- `WitnessInfo::serialize_with_prefix()` re-encodes a new `protocol.Witness` with those fields set to defaults:
  - `pub_key: vec![]`
  - `total_produced: 0`
  - `total_missed: 0`
  - `latest_block_num: 0`
  - `latest_slot_num: 0`
  - `is_jobs: false`

So, when Rust executes a witness update and calls `put_witness(...)`, it can overwrite an existing witness record and reset those fields to defaults. This does **not** match java-tron, where `WitnessUpdateActuator.updateWitness()` only mutates the URL and keeps existing witness stats/metadata intact.

This is consensus-/state-relevant because java-tron updates witness production stats (`total_produced`, `total_missed`, `latest_*`) during block production (e.g. `consensus/src/main/java/org/tron/consensus/dpos/StatisticManager.java`), and witness update should not erase them.

2) **Missing `Any.is(...)` parity check for WitnessUpdateContract**

Java validation includes `any.is(WitnessUpdateContract.class)` and will reject mismatched `Any.type_url` with the `contract type error...` message.

Rust implements the analogous `type_url` check for some contracts (e.g. `WITNESS_CREATE_CONTRACT`), but `execute_witness_update_contract()` currently does not consult `transaction.metadata.contract_parameter` at all.

In normal operation (RemoteExecutionSPI always sends consistent `contract_type` + `contract_parameter`), this won’t surface. But it is a parity gap for malformed requests / conformance fixtures.

### Likely OK, but worth noting (edge cases)

3) **Global “unwrap Any if present” on `tx.data` can theoretically corrupt URL payloads**

`execute_non_vm_contract()` attempts to interpret `transaction.data` as a `google.protobuf.Any` wrapper (by checking for `type.googleapis.com/` in field 1) and replaces `tx.data` with the inner `Any.value` bytes.

For `WITNESS_UPDATE_CONTRACT`, `tx.data` is *not* supposed to be an Any wrapper; it’s the raw `update_url` payload. The current detection is strict enough that this should be extremely unlikely to trigger accidentally, but a crafted `update_url` that is valid Any wire-format would be unwrapped (diverging from Java).

4) **No-op write optimization**

Rust skips persisting the witness if the URL is unchanged; Java always performs `witnessStore.put(...)`. Final state should be the same, but the write/touched-key side effects differ.

---

## Bottom line

Rust’s `WITNESS_UPDATE_CONTRACT` is close on **validation** and basic **“update URL” intent**, but it is **not fully equivalent** to java-tron because it can overwrite the witness record and reset non-URL fields to defaults.

If `witness_update_enabled` is intended to be safe in a mainnet-like environment, the witness persistence path must be changed to update **only** the URL while preserving the rest of the `protocol.Witness` state.

