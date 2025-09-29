> Here’s the first mismatch and why it happens.

- First mismatch
    - tx: 7425ac0022ef1490c7b7d8c533a218d2d4cb6315abfa57486766f694ce6f918c
    - location: block 1990, tx_index 0
    - type: AccountUpdateContract
    - embedded: state_change_count=1, digest=00e13d763969ee1e086e0e9bfa3355ef296a598e4ca92e60abb5a5259d2c4d5c
        - state_changes_json shows only address 414676d81e16604bd8e1dcaad854842699f5ba027e with oldValue==newValue
        - file: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv
    - remote: state_change_count=2, digest=68cc3f5c0285b1c293376c3b3d65a1f30e606664369010b8932a71c38804a2b1
        - extra change at address 410000000000000000000000000000000000000000 (T9yD14… “zero” address), oldValue empty → newValue = zero-account blob; plus the same no‑op for 414676…
        - file: output-directory/execution-csv/20250929-051449-1ee247a1-remote-remote.csv
- Corroborating logs
    - Remote Java shows 2 changes applied, including the zero address:
        - remote-java.e196323.log:64538–64566
            - “Remote execution returned 2 state changes…”
            - “Updating account state… 410000… (T9yD14…) balance 0 -> 0”
            - “Updating account state… 414676d8… balance 3000000 -> 3000000”
- Root cause
    - RemoteExecutionSPI default-maps unhandled contract types to TRANSFER_CONTRACT and builds a request with toAddress = 0x000… and value = 0. There is no explicit case for AccountUpdateContract.
        - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java: buildExecuteTransactionRequest
    - Rust backend’s execute_transfer_contract then:
        - creates a recipient change for the zero address with “new zero account” (oldAccount=None → oldValue empty),
        - adds a no‑op sender change (old=new),
        - rust-backend/crates/core/src/service.rs:292–362
    - ExecutionCsvRecordBuilder prefers execResult.getStateChanges() over the Java journal when present, so the remote’s synthetic zero‑address change replaces the embedded journaled set.
        - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:118–139
- Fix options
    - Safer immediate parity: in RemoteExecutionSPI.buildExecuteTransactionRequest, for any unimplemented non‑VM types (e.g., AccountUpdateContract), do not fall back to TRANSFER_CONTRACT. Either:
        - throw UnsupportedOperationException so remote returns no state changes, and
        - update ExecutionCsvRecordBuilder to fall back to journaled state changes when the ExecutionProgramResult’s stateChanges is empty for non‑VM contracts; or
        - route these contracts to the embedded path (invoke RuntimeImpl/actuators) and keep remote only for VM/explicitly supported non‑VM.
    - Longer term: implement proper AccountUpdateContract handling in the Rust backend and wire a real ContractType → backend mapping in RemoteExecutionSPI, then remove the TRANSFER_CONTRACT default.

Want me to (a) show the minimal code changes to stop the TRANSFER fallback and enable CSV fallback to the journal for non‑VM, or (b) continue to the next mismatching tx?


---


> Objectives

- Handle AccountUpdateContract entirely in Rust (validate, apply, persist).
- Wire explicit ContractType mapping in Java RemoteExecutionSPI; remove TRANSFER fallback.
- Preserve CSV parity: 1 account-level change for owner, no zero-address deltas, energy=0, correct digest.

Java Mapping (RemoteExecutionSPI)

- Map AccountUpdateContract explicitly in framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:
    - Build request with:
        - txKind = NON_VM
        - contractType = ACCOUNT_UPDATE_CONTRACT
        - from = ownerAddress
        - to = None (leave zero-address out)
        - value = 0
        - data = accountName bytes from AccountUpdateContract parameter
    - File refs:
        - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:240
        - Add new switch case alongside TransferContract/Create/Trigger
- Remove default TRANSFER fallback:
    - On unsupported contract types, throw UnsupportedOperationException to fall back to embedded actuators.
    - Ensure log message clearly states the contract type and fallback reason.

Rust Backend: New Handler

- Dispatch
    - Add match arm in execute_non_vm_contract for TronContractType::AccountUpdateContract.
    - File: rust-backend/crates/core/src/service.rs:200–240
- Handler execute_account_update_contract(...) (new function)
    - Parse account name from transaction.data (raw bytes).
    - Validate
        - Non-empty, length ≤ 32 bytes (match java-tron’s constraints), UTF‑8 optional; reject overlong.
        - Owner must exist (load via storage_adapter.get_account(&from)).
        - If protocol parity requires “only set once,” check existing name; reject if already set.
    - Apply
        - Update account name in storage (see Storage support below).
        - No balance/nonce change; energy_used=0; compute bandwidth from payload size.
    - Emit state changes (for CSV parity)
        - Add exactly one account-level state change for owner:
            - old_account = Some(owner_account)
            - new_account = Some(owner_account.clone()) (same values; name is metadata outside AccountInfo)
        - Do NOT emit zero-address or blackhole changes.
    - Return TronExecutionResult with:
        - success = true, return_data = [], energy_used = 0, logs = [], state_changes.len() == 1
    - File: rust-backend/crates/core/src/service.rs (add function and arm).

Storage Support

- Add simple account-name KV in storage module (avoid changing AccountInfo serializer now):
    - DB name: account-name (new).
    - Keys: 21-byte Tron address (0x41 + 20-byte) – reuse account_key() helper.
    - Values: length-delimited UTF‑8 bytes.
- Extend StorageModuleAdapter with:
    - fn get_account_name(&self, address: &Address) -> Result<Option<String>>
    - fn set_account_name(&mut self, address: Address, name: &str) -> Result<()>
    - File: rust-backend/crates/execution/src/storage_adapter.rs
- Optional: add emit_storage_changes support if later you want a storage-level state change for name; keep default off to preserve CSV parity.

CSV Parity Rules

- State changes exported to Java are account-level “AccountInfo blobs” only.
- For AccountUpdateContract:
    - Emit one account change with old==new (length 76 bytes payload) to match embedded’s journaled no-op.
    - Do not emit storage change for name (unless a dedicated flag is introduced later; default false).
- Verify that StateChangeCanonicalizer digest matches embedded when the only change is owner with identical old/new.

Remove TRANSFER Fallback (Java)

- After adding explicit mapping:
    - Delete default contractType = TRANSFER_CONTRACT initialization.
    - For any unhandled contract type, throw and log, letting Manager/RuntimeImpl path execute in Java.
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:200–360

Protocol/Enum Check

- Ensure backend proto enum has ACCOUNT_UPDATE_CONTRACT = 10.
    - If missing, update .proto in backend and regenerate stubs, then map from Java enum accordingly.
- Rust side already supports AccountUpdateContract in enum:
    - rust-backend/crates/execution/src/tron_evm.rs:70–107

Validation & Edge Cases

- Name constraints:
    - Length: enforce ≤32 bytes, allow empty? (match java-tron: typically non-empty; adjust by chain rules if needed).
    - Character set: allow arbitrary bytes or UTF‑8; log and reject invalid UTF‑8 if policy requires.
- “Only set once”:
    - If java-tron allows only once, enforce by checking get_account_name() before setting.
- Bandwidth:
    - Keep energy_used=0 and compute bandwidth from tx payload size; align with current non‑VM rules in service.rs.

Logging

- Rust handler:
    - info: “AccountUpdate for owner TX… name len=…”
    - debug: previous vs new name
    - warn: re-set attempt rejected (if enforced).
- Java mapper:
    - debug: “Mapped AccountUpdateContract to remote request; data_len=…”
    - error: on fallback with reason.

Testing

- Unit (Rust, in-memory adapter):
    - Accept valid name; store in account-name; emit 1 account change old==new; energy 0.
    - Reject overlong name.
    - Reject duplicate set if “only once” enforced.
- Integration (local):
    - Run the CSV harness on a block containing AccountUpdate.
    - Assert:
        - state_change_count parity (1 vs 1).
        - state_digest_sha256 parity for this tx.
        - No zero-address deltas in state_changes_json.
- Java fallback tests:
    - For an intentionally unsupported contract type, verify exception causes fallback to embedded.

Rollout & Flags

- Feature gate in Rust execution.remote.system_enabled already present; add granular account_update_enabled if desired (default true).
- Keep execution.remote.emit_storage_changes=false for parity.
- Document that Java will not reflect account-name in its local AccountStore during state sync (only balance sync is implemented); use embedded path if downstream components need that immediately.

Deliverables

- Java
    - RemoteExecutionSPI mapping for AccountUpdateContract, removal of TRANSFER default.
- Rust
    - New handler execute_account_update_contract.
    - Storage adapter name getters/setters with new account-name DB.
    - Dispatch wiring + tests.
- Docs
    - Update rust-backend/config.toml sample and README: mention account-name storage and parity choices.

Order of Work

1. Add Java mapping + remove TRANSFER fallback.
2. Implement Rust storage adapter name KV.
3. Implement Rust handler + dispatch.
4. Unit tests (Rust).
5. CSV integration check on known AccountUpdate tx.
6. Guardrails (flags), logs, and docs.

Want me to refine constraints (name length/immutability) against java-tron rules before you greenlight this plan?