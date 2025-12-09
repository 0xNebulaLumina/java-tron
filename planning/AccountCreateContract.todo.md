# AccountCreateContract Remote Execution — Implementation Plan

Status: draft
Owners: rust-backend core, execution, storage; java-bridge
Target release: phased (behind flag)

## Summary
Implement AccountCreateContract handling in the Rust backend (remote execution) with byte-level parity goals to Java’s CreateAccountActuator. Feature is guarded by a config flag and mirrors embedded semantics: validate inputs, charge system fee, create the account, optionally credit the blackhole, and emit deterministic state changes with 0 energy and bandwidth accounting.

Key references:
- Java actuator: `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
- Proto: `protocol/src/main/protos/core/contract/account_contract.proto`
- Dispatch site: `rust-backend/crates/core/src/service/mod.rs`
- Engine-backed storage: `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- gRPC conversion: `rust-backend/crates/core/src/service/grpc/conversion.rs`
- Java SPI: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

---

## Parity Requirements (from Java)
- Validate owner address and account address are well-formed TRON addresses.
- Owner account must exist; target account must not exist.
- Owner must have balance >= `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` or fail with message: "Validate CreateAccountActuator error, insufficient fee."
- On execute:
  - Create account capsule (Normal type, 0 balance, create_time now; default permissions may apply on Java side but we do minimal EVM AccountInfo here).
  - Adjust owner balance by system fee.
  - Burn vs credit blackhole based on dynamic properties; record state changes accordingly.
  - Set result status success, fee applied.

Remote result expectations:
- `energy_used = 0`, `bandwidth_used > 0` based on payload size.
- `state_changes` include:
  - Owner AccountChange (old -> new with balance delta)
  - New account AccountChange (None -> new) to mark creation
  - Blackhole AccountChange when crediting (not when burning)
- Deterministic ordering by address (as elsewhere).
- AEXT presence consistent with config (`tracked`/`hybrid`/etc.).

---

## Wire/Protocol Mapping
- Request to Rust backend uses existing `backend.proto`:
  - Set `TronTransaction.contract_type = ACCOUNT_CREATE_CONTRACT`.
  - Encode full `AccountCreateContract` protobuf into `TronTransaction.data`:
    - Use fields: `account_address` (field 2), `type` (field 3). Ignore `owner_address` (we use tx.from).
  - `TxKind = NON_VM`.
- Response mapping already computes `is_creation` based on old/new account presence.

---

## Configuration
Add a feature flag to gate rollout:
- New: `execution.remote.account_create_enabled: bool` (default false)
- Files to update:
  - `rust-backend/crates/common/src/config.rs`:
    - Add field in `RemoteExecutionConfig`.
    - Defaults and environment loading.
  - `rust-backend/config.toml`:
    - Document and set as desired for local/dev.
  - `rust-backend/src/main.rs`:
    - Log flag on startup alongside existing remote flags.

TODOs — Config
- [ ] common/config.rs: add `account_create_enabled: bool` to `RemoteExecutionConfig` with default false
- [ ] config loader: set default key `execution.remote.account_create_enabled=false`
- [ ] config.toml: add commented flag with description
- [ ] main.rs: log flag at startup

---

## Storage Adapter Enhancements
Dynamic property access for fee:
- Implement getter for `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT` (big-endian i64/u64) akin to `get_asset_issue_fee()`.
- Confirm/Reuse existing blackhole helpers:
  - `support_black_hole_optimization()` already present
  - `get_blackhole_address()` already present

Optional improvement (follow-up): serializer currently sets `create_time` using `SystemTime::now()`; parity is better if we can set to `context.block_timestamp` for newly created accounts (requires passing context or altering serializer policy).

TODOs — Storage
- [ ] engine.rs: add `get_create_new_account_fee_in_system_contract() -> Result<u64>`
- [ ] (optional) consider a hook to set account `create_time` deterministically (separate RFC)

---

## Service Dispatch and Handler
Add dispatch entry and a concrete handler.

Dispatch:
- File: `rust-backend/crates/core/src/service/mod.rs`
- In `execute_non_vm_contract`, add match arm for `TronContractType::AccountCreateContract` with feature-flag check (`account_create_enabled`).

Handler: `execute_account_create_contract(...)` (new function)
- Inputs: `&mut EngineBackedEvmStateStore`, `&TronTransaction`, `&TronExecutionContext`
- Steps:
  1. Parse `AccountCreateContract` from `transaction.data`:
     - Accept length-delimited field 2 `account_address` (21-byte TRON addr), field 3 `type` (varint); ignore field 1.
     - Use `grpc::address::strip_tron_address_prefix` to get 20-byte EVM address.
     - Validate address length/format; err on invalid.
  2. Load and validate state:
     - `owner_account = get_account(&tx.from)` must be Some (else: "account <owner> not exist" style message).
     - `get_account(&new_addr)` must be None (else: "Account has existed").
  3. Fee:
     - `fee = get_create_new_account_fee_in_system_contract()`; if `owner.balance < fee` → Err("Validate CreateAccountActuator error, insufficient fee.").
  4. Apply:
     - `new_owner = owner - fee` (U256 math); persist `set_account(tx.from, new_owner)`.
     - `new_account = AccountInfo { balance=0, nonce=0, code_hash=ZERO, code=None }`; persist `set_account(new_addr, new_account)`.
     - Blackhole handling:
       - If `support_black_hole_optimization()` is true → burn fee (no blackhole state change).
       - Else → `bh = get_blackhole_address()`; load bh account (or default empty), add fee, persist; prepare AccountChange for bh.
  5. State changes (Vec<TronStateChange>):
     - Owner AccountChange: (old_owner -> new_owner)
     - New AccountChange: (None -> new_account) to indicate creation
     - Blackhole AccountChange when crediting
     - Sort deterministically by address (match existing sort pattern)
  6. Bandwidth and AEXT:
     - `bandwidth_used = Self::calculate_bandwidth_usage(tx)`
     - If `accountinfo_aext_mode == "tracked"`: load owner AEXT, track bandwidth via `ResourceTracker`, persist, and fill `aext_map[owner] = (before, after)`
  7. Return `TronExecutionResult { success=true, energy_used=0, bandwidth_used, state_changes, aext_map, logs=[], ... }`

TODOs — Service
- [ ] mod.rs: add match arm for `AccountCreateContract` honoring `account_create_enabled`
- [ ] mod.rs: implement `execute_account_create_contract()` per steps above
- [ ] mod.rs: helper to parse `AccountCreateContract` from `transaction.data`
- [ ] contracts/proto.rs: optionally add tiny decoding helpers for this contract (reuse `read_varint`)

---

## gRPC Conversion (Rust)
- Transaction input conversion already supports contract_type mapping; ensure our service uses `transaction.metadata.contract_type`.
- State change conversion already sets `is_creation` based on old==None and new!=None; our new-account state change should set `old_account=None`.

TODOs — gRPC (Rust)
- [ ] Validate no changes required; otherwise, adjust conversion to carry `is_creation` (currently computed via presence).

---

## Java RemoteExecutionSPI wiring
Add support to send AccountCreateContract to Rust backend when remote system contract mode is enabled.

- Map both `CreateAccount` and `CreateAccount2` RPCs to remote execution when configured:
  - In `buildExecuteTransactionRequest(...)`:
    - Add `case AccountCreateContract:`
      - `txKind = TxKind.NON_VM`
      - `contractType = ACCOUNT_CREATE_CONTRACT`
      - `data = contractParameter.toByteArray()` (full serialized proto)
      - `fromAddress = trxCap.getOwnerAddress()`
      - `toAddress = new byte[0]` (unused for this contract)
  - Pre-exec AEXT snapshots: include owner only (target does not exist yet).
  - Keep feature flag gating on Java side if needed (consistent with others: system vs per-contract flags).

TODOs — Java
- [ ] RemoteExecutionSPI: add mapping for `AccountCreateContract` (and `CreateAccount2` wrapper)
- [ ] Ensure pre-exec AEXT collection includes owner
- [ ] Optional: JVM property to quickly disable remote path and fallback to actuator

---

## Tests
Add targeted unit/integration tests; avoid changing unrelated tests.

Rust unit tests (service/contracts):
- [ ] Happy path (burn mode): owner exists, sufficient balance, fee>0, blackhole optimization = true →
  - 2 state changes (owner, new account)
  - energy=0, bandwidth>0, no error
- [ ] Happy path (blackhole credit): same as above with blackhole optimization = false →
  - 3 state changes (owner, new account, blackhole)
  - Verify blackhole balance delta equals fee
- [ ] Insufficient fee → error string matches: "Validate CreateAccountActuator error, insufficient fee."
- [ ] Target account already exists → error: "Account has existed"
- [ ] Owner missing → error: contains "account" and owner address; reconcile final phrasing for parity
- [ ] Invalid account address in data → error about invalid address format
- [ ] Deterministic ordering of state changes by address
- [ ] AEXT tracked mode updates owner AEXT and populates `aext_map`

Rust gRPC conversion tests:
- [ ] `is_creation` flag true for creation change in `ExecuteTransactionResponse`

Java SPI tests (optional, if coverage exists):
- [ ] Build request mapping for AccountCreateContract produces NON_VM, proper contract_type, and data set

---

## Metrics & Logging
- Info logs for create-account execution start/end:
  - owner (base58), new account (base58), fee, blackhole mode
- Debug logs for balances before/after, parsed fields
- Ensure blackhole balance before/after logging path in gRPC service (already present for other flows) can show deltas if feature is enabled

TODOs — Observability
- [ ] Add targeted info/debug logs in handler
- [ ] Verify existing blackhole logging hooks capture this path

---

## Rollout Strategy
- Gate by `execution.remote.account_create_enabled` (default false)
- If disabled, Rust returns a clear error to let Java fall back to embedded actuator
- Phase in on testnets first; monitor CSV parity and state digests

---

## Edge Cases & Compatibility
- Fee=0 networks: allow creation and return owner AccountChange old==new if desired to carry AEXT (or skip; decide and document).
- Address encoding: accept only valid 21-byte TRON with 0x41 prefix in proto; reject malformed bytes.
- Account type: proto `type` ignored initially (serializer emits Normal). Follow-up to reflect type once serializer supports it.
- Create time source: current serializer uses `SystemTime::now()`; parity may be improved by using block timestamp.
- Bandwidth tx size constraints (CREATE_ACCOUNT_TRANSACTION_MIN/MAX_BYTE_SIZE) are enforced in Java BandwidthProcessor; remote path assumes Java-side pre-validation.

---

## Acceptance Criteria
- Remote handler creates an account with correct fee charging and optional blackhole credit.
- Java RuntimeSpiImpl applies state changes to local DB with new account present post-exec.
- Tests pass for happy/negative paths.
- Feature is disabled by default; enabling flag activates handler without impacting other contracts.
- ExecutionResult shows 0 energy, deterministic state changes, and correct `is_creation` in response.

---

## File-by-File TODO Checklist

Rust — common/config
- [ ] `crates/common/src/config.rs`: add `account_create_enabled: bool` to `RemoteExecutionConfig`
- [ ] Defaults in `Config::load()`
- [ ] `impl Default for RemoteExecutionConfig`
- [ ] Log in `src/main.rs`
- [ ] Document in `config.toml`

Rust — storage adapter
- [ ] `crates/execution/src/storage_adapter/engine.rs`: add getter `get_create_new_account_fee_in_system_contract()` reading key `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`

Rust — service core
- [ ] `crates/core/src/service/mod.rs`: add dispatch arm under `execute_non_vm_contract`
- [ ] `crates/core/src/service/mod.rs`: implement `execute_account_create_contract()`
- [ ] `crates/core/src/service/contracts/proto.rs`: small helper to parse `AccountCreateContract` (optional)

Rust — gRPC conversion
- [ ] Verify `conversion.rs` auto-computes `is_creation` correctly; no code changes expected

Java — RemoteExecutionSPI
- [ ] `buildExecuteTransactionRequest(...)`: add `case AccountCreateContract` (+ `CreateAccount2` wrapper path)
- [ ] Ensure owner pre-exec AEXT snapshot included; target account omitted

Tests
- [ ] Add core service tests for AccountCreateContract covering fee modes and errors
- [ ] Add gRPC conversion test for `is_creation`

Docs
- [ ] Update `run.md` or `README.md` with flag usage
- [ ] Note behavioral parity assumptions (create_time, account type)

---

## Risks & Mitigations
- Create time mismatch: document and plan serializer improvement to use block timestamp.
- Type handling: initially ignored; parity impact minimal for core flows; plan follow-up.
- Double-application risk: ensure Java fallback is disabled when flag is on; return explicit error when off.
- CSV parity: enforce deterministic ordering and ensure AEXT mode aligns with existing config.

---

## Open Questions
- Do we need to enforce MAX_CREATE_ACCOUNT_TX_SIZE in Rust? Prefer relying on Java pre-validation to avoid duplication.
- Should we emit a no-op AccountChange for owner when fee=0 to carry AEXT, or skip? Align with existing patterns.

---

## Implementation Outline (Quick Map)
- Config flag: `crates/common/src/config.rs`, `config.toml`, `src/main.rs`
- Fee getter: `crates/execution/src/storage_adapter/engine.rs`
- Dispatch + handler: `crates/core/src/service/mod.rs`
- Proto helper: `crates/core/src/service/contracts/proto.rs`
- Java SPI mapping: `framework/.../RemoteExecutionSPI.java`
- Tests: `crates/core/src/service/tests/contracts.rs`

