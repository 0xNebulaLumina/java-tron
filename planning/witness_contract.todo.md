Title: Remote Non-VM System Contracts Parity Plan (Witness Contracts)

Purpose
- Achieve state-change/digest parity between embedded (Java) and remote (Rust) for system contracts, starting with WitnessCreateContract, and eliminate zero-address artifacts.

Scope (Phase 1 → Phase n)
- Phase 1: Contract-type plumbing, address handling fix, WitnessCreate parity, tests, and validation.
- Phase 2: WitnessUpdate parity.
- Phase 3: VoteWitness parity (subset → complete), storage deltas alignment.
- Out of scope (for now): Full TRC-10 support; advanced governance contracts; archival migrations.

Preconditions / Inputs
- CSV mismatch identified at block 1785/tx 0 (WitnessCreateContract).
- Java actuator source-of-truth: actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121–180.
- Java RemoteExecutionSPI already sets contractType and txKind; ensure toAddress fix.

Milestone Checklist
- [ ] Phase 1 completed: Remote matches embedded for WitnessCreate; first mismatch resolved.
- [ ] Phase 2 completed: Remote matches embedded for WitnessUpdate.
- [ ] Phase 3 completed: Remote matches embedded for VoteWitness (core paths).
- [ ] End-to-end CSV parity verified across targeted blocks.

--------------------------------------------------------------------------------
Phase 1 — WitnessCreate Parity (and zero-address fix)

1) Contract Metadata Plumbing (Rust)
- [ ] Extend internal conversion to carry contract_type and asset_id
  - [ ] service.rs: convert_protobuf_transaction() — parse TronTransaction.contract_type, TronTransaction.asset_id
  - [ ] Define internal enum ContractType mirroring protobuf; add TryFrom/i32 mapping
  - [ ] Add TxMetadata { tx_kind, contract_type, asset_id: Option<Vec<u8>> }
- [ ] Thread metadata to Non-VM dispatcher
  - [ ] execute_transaction() — pass tx_kind + contract_type to Non-VM branch
  - [ ] Replace execute_non_vm_transaction(...) with execute_non_vm_dispatch(...)

2) Address Handling Fix
- [ ] Java: Stop using to=0x00.. for system contracts
  - [ ] RemoteExecutionSPI: For WitnessCreate/Update/Vote set .setTo(ByteString.EMPTY)
  - [ ] Keep txKind=NON_VM, setContractType(...), setAssetId(...) only when applicable
- [ ] Rust: Allow transaction.to=None for system contracts
  - [ ] Non-VM dispatcher must not require `to` except for TRANSFER_CONTRACT/TRANSFER_ASSET_CONTRACT

3) Dynamic Properties Access (Rust)
- [ ] Define property accessors in storage engine/adapter
  - [ ] get_account_upgrade_cost() (AccountUpgradeCost)
  - [ ] get_allow_multi_sign() (AllowMultiSign)
  - [ ] support_black_hole_optimization() (SupportBlackHoleOptimization)
  - [ ] get_blackhole_address() (when crediting)
- [ ] Ensure defaults match embedded at block heights under test
  - [ ] Document canonical keys and default values

4) Witness Store Support (Rust Storage)
- [ ] Add CF/namespace for WitnessStore (by 21-byte Tron address key)
- [ ] Define serialization for WitnessCapsule-equivalent { address, url, voteCount }
- [ ] Implement get_witness(addr), put_witness(witness)
- [ ] Unit-test roundtrip (serialize/deserialize)

5) Account Serialization Parity (Rust→Java Bridge)
- [ ] Ensure empty-code normalization to keccak("") (c5d246…a470) is preserved
- [ ] Keep account serialization format (balance[32] + nonce[8] + code_hash[32] + code_len[4] + code)
- [ ] Verify RemoteExecutionSPI.serializeAccountInfo() remains the consumer of AccountChange data

6) Implement WitnessCreate Handler (Rust)
- [ ] Validate preconditions
  - [ ] Owner account exists
  - [ ] Owner not already a witness
  - [ ] URL validity check (match embedded or accept as-is initially; log deviations)
  - [ ] Balance >= AccountUpgradeCost
- [ ] Mutations
  - [ ] Insert witness entry: (owner, url, voteCount=0)
  - [ ] Update owner account flags: isWitness=true; default witness permission if allowMultiSign==1
  - [ ] Deduct AccountUpgradeCost from owner balance
  - [ ] Burn or credit cost:
    - [ ] If supportBlackHoleOptimization() -> burn (no account delta)
    - [ ] Else -> credit blackhole account +cost (AccountChange)
  - [ ] Increment total create witness cost (DynamicProperties)
- [ ] Emitted state changes (for CSV parity)
  - [ ] Owner AccountChange: balance decreased (old/new)
  - [ ] Owner AccountChange: metadata/permission changed (ensure serialized blob reflects change)
  - [ ] Optional blackhole AccountChange only when not burning
  - [ ] No zero-address AccountChange
- [ ] Deterministic ordering: sort by address asc; account before storage for same address

7) Logging and Metrics
- [ ] Log chosen Non-VM branch: WitnessCreateContract
- [ ] Log cost, burn vs blackhole credit, resulting owner balance
- [ ] Avoid logging zero-address creation anywhere

8) Feature Flags & Fallback
- [ ] Introduce flags (readable by Java and/or Rust)
  - [ ] remote.exec.system.enabled=true (globally enable system contracts)
  - [ ] remote.exec.witness.create.enabled=true (default true)
- [ ] Java: If disabled or error encountered, throw to fall back to Java actuator

9) Tests (Rust Unit + Integration)
- [ ] Unit: convert_protobuf_transaction() parses contract_type and allows to=None
- [ ] Unit: Witness store read/write
- [ ] Unit: Dynamic properties defaulting
- [ ] Integration: Execute WitnessCreate with fixture context
  - [ ] Emitted changes count matches embedded (2 owner changes; +blackhole if mode == credit)
  - [ ] Account blobs lengths 76 and code hash equals keccak("")
  - [ ] Deterministic order; digest parity script passes for the tx

10) End-to-End Validation
- [ ] Re-run remote-remote on blocks up to and including 1785
- [ ] Confirm first mismatch disappears
- [ ] Capture CSV diff and update notes

--------------------------------------------------------------------------------
Phase 2 — WitnessUpdate Parity

1) Semantics & Validation
- [ ] Mirror WitnessUpdateActuator (owner must be witness; url validation)

2) Mutations
- [ ] Update WitnessStore URL (deterministic serialization)
- [ ] No balance change; no zero-address changes

3) Emitted Changes
- [ ] If embedded surfaces only account-level change, follow suit (document)
- [ ] If storage changes must be emitted: produce StorageChange with deterministic key format aligned to Java DB keys

4) Tests & Validation
- [ ] Unit: witness update write/read
- [ ] Integration: target a block/tx with WitnessUpdate; parity on state_digest

--------------------------------------------------------------------------------
Phase 3 — VoteWitness Parity

1) Scope & Rules
- [ ] Mirror VoteWitnessActuator + VoteWitnessProcessor core behavior
- [ ] Validate witness list, arrays lengths, vote caps, and constraints

2) Mutations
- [ ] Update voter’s vote mapping
- [ ] Update witness vote tallies/vi and any aggregates (per-cycle where applicable)

3) Emitted Changes
- [ ] AccountChange for voter if metadata or balance affected
- [ ] StorageChange(s) for tallies with deterministic keys
- [ ] Preserve ordering rules

4) Tests & Validation
- [ ] Unit: tally math paths
- [ ] Integration: known VoteWitness tx samples; digest parity

--------------------------------------------------------------------------------
Cross-Cutting Tasks

Serialization & Keys
- [ ] Document deterministic key formats for new storage entries (WitnessStore, vote tallies)
- [ ] Ensure big-endian encoding matches Java conventions

Error Handling & Fallback
- [ ] On validation failure in Rust Non-VM handler, return explicit error; Java should fallback to actuator
- [ ] Log structured errors with tx_id, block_num

Configuration & Defaults
- [ ] Review rust-backend/config.toml options for flags
- [ ] Add env overrides for dynamic properties during tests

Observability
- [ ] Add tracing spans for Non-VM dispatcher and specific contract handlers
- [ ] Summarize state change counts and addresses for audit

Documentation
- [ ] Update docs/ or planning/ notes with implemented parity coverage and known gaps

--------------------------------------------------------------------------------
Validation Playbook
- [ ] Run block 1785 replay (remote-remote) and compare to embedded CSV
- [ ] Verify state_changes_json matches structure and lengths (two 76-byte account blobs, no zero-address)
- [ ] Verify state_digest_sha256 parity at row 7
- [ ] Expand sample set to next N blocks with witness-related txs

Exit Criteria
- [ ] First mismatch removed; digest parity for targeted witness txs
- [ ] No zero-address account creations in logs or CSV
- [ ] Tests stable and deterministic

Risks & Mitigations
- [ ] Dynamic property drift — pin test values or snapshot from Java DB
- [ ] Storage key mismatch — gate storage changes emission until verified
- [ ] VoteWitness complexity — stage rollout; start with basic scenarios

Rollout & Backout
- [ ] Default enable WitnessCreate only; others behind flags
- [ ] Backout path: disable flags; Java actuators take over

