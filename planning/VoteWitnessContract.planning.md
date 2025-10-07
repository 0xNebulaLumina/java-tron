- Extend Rust backend NonVm handling to branch on contract_type and implement the actuator semantics for VoteWitnessContract
    - Use the logic in actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java as the source of truth

---


Here’s a concrete, code‑ready plan to extend the Rust backend NonVM path to handle VoteWitnessContract with actuator‑accurate semantics.

Goal

- Execute VoteWitnessContract in the Rust backend’s NonVM path, matching java‑tron’s actuator semantics as defined in actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:34.
- Accept full VoteWitnessContract proto bytes from Java (RemoteExecutionSPI already sends this), validate and apply state changes (votes store + deterministic account change for CSV parity), and return a 0‑energy,
NonVM result.

Entry Points

- Dispatch already exists: rust-backend/crates/core/src/service.rs:269 routes TronContractType::VoteWitnessContract to execute_vote_witness_contract(); the method is currently a TODO at rust-backend/crates/core/
src/service.rs:693.
- Java already serializes and forwards VoteWitnessContract in data and tags the tx correctly:
    - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:394
    - framework/src/main/proto/backend.proto:516 (contract_type in TronTransaction)
    - protocol/src/main/protos/core/contract/witness_contract.proto:19 (VoteWitnessContract)

Data Model & Storage Additions

- Votes DB
    - Add StorageModuleAdapter support for database "votes":
        - votes_key(address) → 21‑byte Tron address (0x41 prefix + 20 bytes), same as account keys.
        - VotesRecord { address, old_votes: Vec<(Address,u64)>, new_votes: Vec<(Address,u64)> }.
        - serialize/deserialize VotesRecord to match protocol.Votes wire format exactly:
            - Votes { bytes address = 1; repeated Vote old_votes = 2; repeated Vote new_votes = 3 }
            - Vote { bytes vote_address = 1; int64 vote_count = 2 }
        - Methods: get_votes(&Address) -> Result<Option<VotesRecord>>, set_votes(Address, &VotesRecord) -> Result<()>.
    - File: rust-backend/crates/execution/src/storage_adapter.rs: add DB name “votes” helpers and new methods near existing witness and freeze helpers (around 240, 560).
- Dynamic properties accessors:
    - support_allow_new_resource_model() via key "ALLOW_NEW_RESOURCE_MODEL".
    - support_unfreeze_delay() via key "UNFREEZE_DELAY_DAYS" > 0.
    - File: rust-backend/crates/execution/src/storage_adapter.rs: add methods next to existing getters (around 460+).
- Tron Power calculator:
    - Provide helper get_tron_power_in_sun(address, new_model: bool) -> Result<u64>.
    - Phase 1: sum TRON_POWER from freeze ledger (resource = 2) via get_freeze_record/add_freeze_amount; treat that as total voting power in SUN.
    - Phase 2 (parity): incorporate “oldTronPower” semantics and frozen V1/V2 fields by parsing Account protobuf; compute getAllTronPower() parity per chainbase/src/main/java/org/tron/core/capsule/
    AccountCapsule.java: getAllTronPower and related helpers (around 1080+).
    - File: rust-backend/crates/execution/src/storage_adapter.rs.

Parsing & Validation

- Parse VoteWitnessContract data
    - Implement a small parser for protocol.VoteWitnessContract (owner_address:1, repeated Vote:2, support:3), using the existing varint utilities (rust-backend/crates/core/src/service.rs:30) to read field tags/wire
    types and nested messages.
    - Extract:
        - owner_address (optional; use transaction.from as canonical owner)
        - votes: Vec<(vote_address, vote_count)>
        - support (bool; not used by actuator)
    - File: rust-backend/crates/core/src/service.rs: add parse_vote_witness_contract(&[u8]) helper near other parsing code.
- Validate exactly like VoteWitnessActuator (actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:72, 88, 100, 128)
    - any not null (implicit by our parsing)
    - votes_count > 0 and <= MAX_VOTE_NUMBER (30) from common/src/main/java/org/tron/core/config/Parameter.java:66
    - each vote:
        - vote_address valid Tron address (length 21, 0x41 prefix)
        - vote_count > 0
        - accountStore.has(vote_address) and witnessStore.has(vote_address) must both be true
            - Use StorageModuleAdapter.get_account(vote_address) presence as account existence
            - Use StorageModuleAdapter.get_witness(vote_address) for witness existence
    - sum = Σ(vote_count) in TRX units; convert to SUN by sum * TRX_PRECISION (1_000_000; common/src/main/java/org/tron/core/config/Parameter.java:81) via checked math.
    - tronPower check:
        - new_model = support_allow_new_resource_model()
        - tronPowerSUN = get_tron_power_in_sun(owner, new_model)
        - Require sum*TRX_PRECISION <= tronPowerSUN; otherwise fail.

Notes:

- We use VoteWitnessActuator as the validation source of truth; VoteWitnessProcessor (VM-native) has lighter checks and dedup; we should not mimic that here.

Execution Semantics

- Withdraw reward
    - VoteWitnessActuator withdraws rewards first (actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:158).
    - Phase 1: log “skipped withdrawReward: remote delegation not yet ported” and proceed; this defers allowance changes but keeps voting deterministic.
    - Phase 2: port MortgageService delegation store to Rust and implement reward withdrawals.
- Votes update
    - Load existing votes record: get_votes(owner) or create VotesRecord with old_votes from prior record’s new_votes (or parse Account votes list in Phase 2).
    - Clear owner account’s “votes list” in the VotesRecord scope: votes_record.clear_new_votes().
    - For each validated vote: votes_record.add_new_vote(vote_address, vote_count).
    - Persist with set_votes(owner, &votes_record).
    - Optionally update account’s embedded Account.votes list:
        - Phase 2: parse Account protobuf from “account” DB, replace repeated votes (field 5), and write back — matching actuator’s accountCapsule behavior.
- Output result
    - energy_used = 0
    - bandwidth_used = payload_size heuristic (already available at rust-backend/crates/core/src/service.rs:1017)
    - state_changes:
        - Emit one deterministic AccountChange for owner (old_account/new_account same) for CSV parity, like other NonVM handlers:
            - rust-backend/crates/core/src/service.rs:818 example pattern.
        - Do NOT emit arbitrary storage deltas for “votes” DB; Java will read “votes” from Rust storage via the SPI (StorageBackendDB). Keep CSV parity stable.

Config & Flags

- Gate execution behind remote.vote_witness_enabled:
    - Already wired (rust-backend/crates/core/src/service.rs:265).
- Use execution.remote.emit_storage_changes ONLY if in the future we want to emit additional deltas; default remains false to avoid CSV drift (rust-backend/crates/common/src/config.rs:91).
- Keep non_vm_blackhole_credit_flat out of votes (fee 0 for system contract); calcFee() is zero in actuator.

Errors & Messages

- On invalid address: “Invalid address”
- On invalid vote address: “Invalid vote address!”
- On missing account or witness: match VoteWitnessActuator messages:
    - “account <addr> not exist”
    - “Witness <addr> not exist”
- On counts: “vote count must be greater than 0”; “VoteNumber must more than 0”; “VoteNumber more than maxVoteNumber 30”
- On power: “The total number of votes[sum] is greater than the tronPower[tp]”
- Use consistent readable Tron addresses in logs (rust-backend/crates/core/src/service.rs helpers add_tron_address_prefix and to_tron_address).

Testing Plan

- Unit tests in Rust (add to rust-backend/crates/core/src/tests.rs)
    - test_vote_witness_success_basic:
        - Setup: put_witness for a few addresses; set freeze ledger TRON_POWER so owner has adequate power; enable vote_witness_enabled; craft VoteWitnessContract bytes with 2 votes; TxKind=NON_VM;
        ContractType=VOTE_WITNESS_CONTRACT.
        - Expect: success, energy_used 0, one AccountChange for owner, votes DB updated: old_votes reflect previous, new_votes correct.
    - test_vote_witness_exceeds_max_votes:
        - 31 votes → error “VoteNumber more than maxVoteNumber 30”
    - test_vote_witness_invalid_vote_count_zero:
        - A vote with count 0 → error “vote count must be greater than 0”
    - test_vote_witness_missing_witness:
        - A vote to non‑witness → error “Witness <addr> not exist”
    - test_vote_witness_over_power:
        - sum(TRX)*TRX_PRECISION > tronPowerSUN → error with power message.
    - Parser tests:
        - Roundtrip serialize/deserialize VotesRecord with multiple votes.
        - Parse VoteWitnessContract with/without support field; ignore owner in payload and use transaction.from.
- Java integration footprint
    - RemoteExecutionSPI mapping for VoteWitness already implemented (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:394).
    - Validate end‑to‑end by enabling remote and running focused tests:
        - ./gradlew :framework:test --tests "org.tron.core.actuator.VoteWitnessActuatorTest" with STORAGE_MODE=remote and backend enabled to verify observable parity in stores (reads go to remote via SPI).
    - Optional: add golden vector(s) for votes in framework/src/test/resources/golden-vectors.json and loader in GoldenVectorLoader when votes support is complete (currently marked unsupported at framework/src/test/
    java/org/tron/core/execution/spi/GoldenVectorLoader.java:258).

Rollout & Backward Compatibility
    - Skip withdrawReward (log only).
- Phase 2 (full parity):
    - Parse TRON Account protobuf to:
        - Read/maintain votes list in Account; write back updated votes
        - Read oldTronPower and frozen balances to compute getAllTronPower exactly
    - Implement withdrawReward by porting delegation store primitives or simulate allowance deltas.
    - Add negative tests for reward/vote cycles if delegation is ported.

Files To Touch

- rust-backend/crates/core/src/service.rs:693
    - Implement execute_vote_witness_contract()
    - Add parse_vote_witness_contract(), constants for MAX_VOTE_NUMBER and TRX_PRECISION, and use existing bandwidth calc.
- rust-backend/crates/execution/src/storage_adapter.rs: add votes DB helpers; dynamic property getters; tron power helper; VotesRecord serializer/deserializer.
- rust-backend/crates/common/src/config.rs: no changes required; flags already present.
- protocol references for correctness:
    - protocol/src/main/protos/core/contract/witness_contract.proto:19

Risks & Mitigations

- Account votes list parity:
    - Phase 1 skips mutating Account’s repeated votes; consensus and reward logic rely on VotesStore; reading account votes list may not reflect updated votes until Phase 2. Mitigate by prioritizing Phase 2 in next
    iteration.
- Withdraw reward timing:
    - Skipped in Phase 1; doesn’t affect consensus outcome for voting action but affects live allowance visibility; document behavior and guard with logs.
- Serialization correctness:
    - Must exactly match protocol.Votes protobuf; write roundtrip tests in Rust to ensure Java VotesCapsule(byte[]) can parse our writes.

Logging & Metrics

- Log owner, number of votes, sum in TRX and SUN, computed tronPower, and result.
- On error, emit actuator‑consistent messages.
- Respect existing tracing patterns in service.rs and storage_adapter.rs.

If you want, I can turn this plan into a short implementation checklist in the repo, and then wire up the tests for Phase 1 to validate behavior end‑to‑end.

