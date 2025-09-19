Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv
+ output-directory/execution-csv/20250918-104622-ff094c1e-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 5.embedded-java.log
(remote execution + remote storage) java log: remote-java.5f57c3b.log
(remote execution + remote storage) rust log: remote-rust.5f57c3b.log

---

Here’s the first mismatch and why it happens.

- First mismatch
    - Tx: block 1785, index 0, tx_id 18bd4e3bb4552df2d2d24821bddf7079df9bf7f293978c97d2c7c8613d45b3e3
    - Field diff: state_digest_sha256
        - embedded: 21d448c562bec60e94ef30caa9a7b30950f68c97e60ed67b31885de3e1283909
        - remote:   9f86efa04643897f32edf2ccabcbd1e165aaa3c04f8c2516e6157c9251201918
- What changed on each side
    - Embedded (account-level changes, 2 entries)
        - Address 4116329c4b64920408342e66221a1d52a974901984 (owner) changed; balance delta equals witness creation cost.
        - Address 4177944d19c052b73ee2286823aa83f8138cb7032f changed; this is a second account-level mutation associated with witness registration (account flags/permissions).
        - Source: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv
    - Remote (account-level changes, 2 entries)
        - Address 410000000000000000000000000000000000000000 “created” with zero balance/code (base58 T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb).
        - Address 4116329c4b64920408342e66221a1d52a974901984 (owner) recorded but oldValue == newValue (no effective change).
        - Source: output-directory/execution-csv/20250918-104622-ff094c1e-remote-remote.csv
- Evidence in logs
    - Remote Java shows it applying state changes for this tx and creating the zero address:
        - remote-java.5f57c3b.log:63921–63930
            - 63921–63922: Remote execution returned 2 changes; address lengths 21
            - 63924–63926: Updating account 4100… (T9yD14N…), Created new account … with balance 0
            - 63927–63929: Updating owner 4116…; balance 15000000000 -> 15000000000 (unchanged)
    - Embedded Java only shows block processing; it doesn’t print per-change detail for this path.
- Root cause
    - Java sends non-VM contracts to the remote with toAddress set to all-zero (20 bytes) by default.
        - See framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:269–281, 331–338, 350–359
            - toAddress defaults to new byte[20] for “other” contract types (including WitnessCreateContract).
    - Rust backend treats all NonVm txs as a value transfer from from → to, requiring a “to” address.
        - It accepts the zero address as a valid “to” and emits an AccountChange for 0x000… (T9yD14N…).
        - See rust-backend/crates/core/src/service.rs:181–199 (execute_non_vm_transaction) and 1888–1964 (convert_protobuf_transaction).
    - The remote path does not implement WitnessCreateContract semantics. Embedded does:
        - WitnessCreateActuator updates witness store and marks owner as witness, deducts upgrade cost and burns/credits blackhole.
        - See actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121–180
- Why the digest diverges
    - Remote emits: [zero-address creation, owner unchanged] → different state set → different state_digest_sha256.
    - Embedded emits: [owner debited + witness/account flag change] → expected digest.
- Recommended next steps
    - Short-term guard (to restore parity quickly):
        - In RemoteExecutionSPI, gate remote execution to an allowlist of supported contract types. For now:
            - Remote only: TransferContract (and optionally TRC-10 TransferAsset when explicitly enabled).
            - For others (e.g., WitnessCreateContract), throw an UnsupportedOperationException so the node uses embedded actuators. Reference: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:331–338.
    - Medium-term fix:
        - Extend Rust backend NonVm handling to branch on contract_type and implement the actuator semantics for:
            - WitnessCreateContract, WitnessUpdateContract, VoteWitnessContract, etc.
            - Use the logic in actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121–180 as the source of truth (update witness store, owner flags, and fee burn/credit).
        - Also stop setting toAddress=0x00 for non-transfer system contracts; either leave to empty or set a contract_type and let backend dispatch.

If you want, I can:

- Add the Java guard to fall back to embedded for unsupported non-VM contracts, so remote runs stop producing zero-address artifacts.
- Or sketch the Rust-side contract-type dispatch for WitnessCreateContract to produce the same state deltas as embedded.

---

think harder.

ok, no need to code, give me a detailed plan on:
- Extend Rust backend NonVm handling to branch on contract_type and implement the actuator semantics for:
    - WitnessCreateContract, WitnessUpdateContract, VoteWitnessContract, etc.
    - Use the logic in actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121–180 as the source of truth (update witness store, owner flags, and fee burn/credit).
- Also stop setting toAddress=0x00 for non-transfer system contracts; either leave to empty or set a contract_type and let backend dispatch.

---


Goal

- Make remote NonVm execution produce the same deterministic state changes as embedded for system contracts (e.g., WitnessCreate/Update/Vote), and avoid spurious zero-address mutations.
- Stop zero-address “to” placeholders for non-transfer system contracts; dispatch by explicit contract_type instead.

Overview

- Add explicit contract_type-driven dispatch in Rust for NonVm transactions.
- Model Java actuators’ semantics in Rust for WitnessCreateContract first, then WitnessUpdateContract and VoteWitnessContract.
- Align address handling, account serialization, fee/cost burning and state-change ordering with embedded.
- Gate by feature flags; add targeted tests to confirm CSV digest parity.

Protocol & Metadata

- Parse and carry contract_type end-to-end.
    - Rust: Extend conversion to read TronTransaction.contract_type and asset_id.
        - File: rust-backend/crates/core/src/service.rs:1888 (convert_protobuf_transaction)
    - Represent in an internal TxMetadata alongside TxKind and optional asset_id.
- Do not infer system-contract behavior from to or data; use contract_type.

Java Front-End Adjustments

- Stop setting toAddress=0x00 for non-transfer system contracts.
    - RemoteExecutionSPI: For WitnessCreate/Update/Vote, set to to empty bytes (ByteString.EMPTY) and only set contractType.
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:348–360, 361–385
- Ensure proper classification and context are already set:
    - setTxKind(TxKind.NON_VM), setContractType(...), setAssetId(...) (TRC-10 only).
    - Confirm block context comes from BlockCapsule (already implemented).
- Optional safety: For unsupported system contracts, throw to force Java actuators (gradual rollout).

Rust Execution Dispatcher

- Split NonVm execution into a dispatcher:
    - Convert execute_non_vm_transaction(...) into execute_non_vm_contract(contract_type, …).
    - Branches:
        - TRANSFER_CONTRACT → existing TRX transfer path (requires to).
        - TRANSFER_ASSET_CONTRACT (TRC-10) → optionally later; leave off by default or behind flag.
        - WITNESS_CREATE_CONTRACT → implement per Java actuator.
        - WITNESS_UPDATE_CONTRACT → implement per Java actuator.
        - VOTE_WITNESS_CONTRACT → implement per Java/native contract path.
    - File: rust-backend/crates/core/src/service.rs:181–220 (rename/refactor), 1553–1571 (dispatcher site).

WitnessCreateContract Semantics (Parity with Java)

- Source of truth: actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121
- Validate:
    - Owner account exists; not already witness; URL is valid; balance ≥ AccountUpgradeCost; read allowMultiSign.
- State mutations:
    - WitnessStore.put(OwnerAddress, WitnessCapsule(url, voteCount=0)).
    - AccountStore.put(owner): setIsWitness(true); set default witness permission if allowMultiSign == 1.
    - Deduct AccountUpgradeCost from owner balance.
    - Burn or blackhole-credit cost:
        - If DynamicProperties.supportBlackHoleOptimization: burn (supply reduction, no account delta).
        - Else: credit blackhole account with +cost.
    - DynamicProperties.addTotalCreateWitnessCost(cost).
- Emitted state changes (for CSV parity):
    - Owner AccountChange with old/new (balance decreased).
    - Owner AccountChange reflecting witness flag/permission delta (ensure encoded in 76-byte account blob).
    - Do not emit zero-address AccountChange.
    - Preserve current 76-byte account serialization contract used by RemoteExecutionSPI.
- Determinism:
    - Sort state_changes by address ascending (accounts before storage), as already implemented.
- Storage integration:
    - Add Witness store CF and get/put helpers to the storage engine and adapter.
    - Add DynamicProperties reads (AccountUpgradeCost, supportBlackHoleOptimization, allowMultiSign).
        - Use existing properties DB; define canonical keys and default values matching embedded at that height.

WitnessUpdateContract Semantics

- Source: actuator/src/main/java/org/tron/core/actuator/WitnessUpdateActuator.java
- Validate:
    - Owner is a witness; URL valid; permissions if required.
- State mutations:
    - Update WitnessStore entry with new URL.
- Emitted state changes:
    - If embedded logs record only account deltas, do not emit extraneous storage deltas.
    - Otherwise, emit a StorageChange keyed deterministically for the witness store (flag behind parity feature).
- No balance change; ensure no zero-address changes.

VoteWitnessContract Semantics

- Source: actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java and VM-native VoteWitnessProcessor (actuator/src/main/java/org/tron/core/vm/nativecontract/VoteWitnessProcessor.java).
- Validate:
    - Owner account exists; vote arrays valid; referenced witnesses must exist; voting constraints per rules.
- State mutations (high level):
    - Update voter’s vote mappings; update witnesses’ vote tallies/vi; adjust any affected accumulators.
    - Update any per-cycle or per-period aggregates consistent with Java stores.
- Emitted state changes:
    - AccountChange(s) if voter’s account metadata or balances change.
    - StorageChange(s) for vote tallies keyed deterministically (replicate Java’s repository keys).
- Phase plan:
    - Phase 1: implement minimal subset that covers the CSV rows encountered next after WitnessCreate (to de-risk and validate parity).
    - Phase 2: complete all edge rules and lifecycle effects.

Address Handling Fix

- Java: For system contracts (WitnessCreate/Update/Vote), set to = empty, not 0x00… (avoid T9yD14N… creation).
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:348–360
- Rust: Allow transaction.to = None for system contracts; do not require to in dispatcher except for transfers.
    - File: rust-backend/crates/core/src/service.rs:194 (remove hard require for NonVm); pass contract_type instead.

Dynamic Properties & Fees

- Read dynamic flags and costs from storage:
    - AccountUpgradeCost, supportBlackHoleOptimization, allowMultiSign, blackhole address (if crediting).
- Align fee/cost accounting:
    - Keep NonVm “flat fee” off by default (already None).
    - Witness cost handling per Java is not a gas/energy fee; process explicitly as above.

Serialization & Digest Parity

- Reuse existing account serialization for AccountChange:
    - 32-byte balance, 8-byte nonce, 32-byte empty-code keccak (c5d2…a470), 4-byte code length, code bytes.
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:420–520 (serializeAccountInfo)
- Zero-code normalization must match on Rust→Java bridge (already enforced).
    - File: rust-backend/crates/core/src/service.rs:2048+ (convert_execution_result_to_protobuf)
- Preserve state change ordering and counts to match embedded CSV digest.

Storage Engine Extensions

- Add/get/put APIs:
    - WitnessStore CF: by 21-byte address key; value = serialized WitnessCapsule (address, url, voteCount).
    - DynamicPropertiesStore CF: keys for required properties with defaults.
- Provide adapter methods in execution/storage_adapter to record state_change_records for:
    - AccountChange (existing).
    - StorageChange for witness/vote updates (new deterministic key scheme aligned with Java’s DB key format).

Logging & Telemetry

- Add logs for:
    - Parsed contract_type and chosen NonVm branch.
    - WitnessCreate: cost, burn/credit mode, target balances, witness store entry.
    - Elide zero-address creation logs entirely.
- Sample in remote-java logs already print per-change sizes; keep for verification.

Feature Flags & Rollout

- Flags:
    - remote.exec.system.enabled (default true for WitnessCreate only initially).
    - remote.exec.witness.update.enabled, remote.exec.vote.enabled (default false; enable progressively).
    - remote.exec.emit.storage_changes (default off if embedded CSV doesn’t record them).
- Java fallback:
    - If disabled/unsupported, throw in RemoteExecutionSPI to fall back to embedded actuators.

Testing & Validation

- Unit tests (Rust):
    - Convert_protobuf_transaction parses contract_type and leaves to=None for system contracts.
    - WitnessCreate handler validates, mutates account and witness store, emits 2 account changes in correct order.
- Integration tests:
    - Replay block 1785/tx 0 context to produce identical state_digest as embedded at row 7.
    - Add cases for blackhole optimization on/off; verify owner balance and burn vs credit behavior.
- Cross-run CSV diff:
    - Re-run “remote-remote” end-to-end; confirm first mismatch disappears; record state_digest parity.

Risks & Mitigations

- Dynamic properties drift:
    - Ensure backend initializes/maintains same values as embedded for the tested height; add property sync or config snapshot.
- Storage key mismatch:
    - Mirror Java key encoding for witness and vote stores; feature-flag emitting storage changes until parity is confirmed.
- Scope creep (VoteWitness complexity):
    - Ship WitnessCreate first (fixes the earliest mismatch), then expand.

Deliverables

- Rust: contract_type-driven NonVm dispatcher; WitnessCreate implemented with storage + account changes.
    - Files: rust-backend/crates/core/src/service.rs, rust-backend/crates/execution/src/storage_adapter.rs, rust-backend/crates/storage/src/engine.rs
- Java: RemoteExecutionSPI stops zero-address for system contracts; keeps contract_type and NonVm classification.
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:261–420
- Tests: parity tests for block 1785/tx 0 and a small suite for WitnessCreate variants.
- Validation: New remote CSV matches embedded digest at the first mismatch; confirm via diff script.