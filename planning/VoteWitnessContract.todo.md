# VoteWitnessContract Remote NonVM Execution — Detailed Plan & TODOs

Owner: Rust Backend Team
Status: Draft (Phase 1 → Phase 2)
Scope: Implement actuator-accurate NonVM handling for `VoteWitnessContract` in the Rust backend, gated by `execution.remote.vote_witness_enabled`.

---

## Context & Goals

- Route TRON system contract `VoteWitnessContract` (ContractType=4) to a NonVM handler in Rust that mirrors java-tron’s actuator semantics.
- Source of truth for behavior: `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java` (validation + count logic), not the TVM-native processor.
- Preserve CSV parity (deterministic state_changes format), zero energy usage, feature-flagged rollout.

Key Java references:
- Actuator logic: `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java`
- VM-native processor (secondary reference): `actuator/src/main/java/org/tron/core/vm/nativecontract/VoteWitnessProcessor.java`
- Protobuf: `protocol/src/main/protos/core/contract/witness_contract.proto`
- Dynamic properties semantics: `chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java`

Rust backend entry points:
- Dispatch: `rust-backend/crates/core/src/service.rs` → `execute_non_vm_contract()` branches to `execute_vote_witness_contract()` when `contract_type=VoteWitnessContract` and `execution.remote.vote_witness_enabled`.
- Storage adapter: `rust-backend/crates/execution/src/storage_adapter.rs` (witness store exists; add votes store + dynamic properties helpers + tron power computation helpers).

---

## High-Level Behavior (Actuator Parity)

Validation (must match Actuator):
- Contract present and of expected type.
- `votes_count > 0` and `votes_count <= MAX_VOTE_NUMBER (30)`.
- For each vote:
  - `vote_address` is a valid Tron address (21 bytes, 0x41 prefix) and exists as an account.
  - `vote_address` exists in `WitnessStore`.
  - `vote_count > 0`.
- Total votes in TRX multiplied by `TRX_PRECISION` (1_000_000) must be <= `tronPower` of owner, where `tronPower = getAllTronPower()` if `ALLOW_NEW_RESOURCE_MODEL=1`, else `getTronPower()`.

Execution (actuator-style):
- Withdraw reward for owner before applying new votes (Phase 2; Phase 1 logs and skips).
- Load/create `VotesCapsule` for owner: initialize with `oldVotes=account.votesList`, then `clearNewVotes` and `account.clearVotes`.
- For each (vote_address, vote_count) in payload: append to `votesCapsule.newVotes` and to `account.votesList` (Phase 2 keeps Account’s list; Phase 1 may only mutate votes DB).
- Persist: `VotesStore.put(owner, votesCapsule)` and `AccountStore.put(owner, account)`.
- Result: success, `energy_used=0`, `bandwidth_used` computed from payload, and exactly one deterministic `AccountChange` for owner (old==new) to keep CSV parity.

Notes:
- Error messages should mirror actuator wording for test parity (e.g., invalid address, witness not exist, vote count > 0, total votes > tronPower, etc.).
- Do not emit raw storage deltas for votes by default; Java reads from Rust storage via SPI. Keep `emit_storage_changes` as future flag.

---

## Data & Storage Plan

Databases used (existing / to add):
- `account` (existing): serialized TRON Account protobuf.
- `witness` (existing): `WitnessInfo` serialization in adapter; used for existence checks.
- `votes` (add): serialize `protocol.Votes` for each owner.
  - Key: 21-byte TRON address (0x41 + 20; same convention as account DB).
  - Value: `Votes { address, repeated Vote old_votes, repeated Vote new_votes }`.

Dynamic Properties (read-only in Phase 1):
- `ALLOW_NEW_RESOURCE_MODEL` (boolean): determines tron power source.
- `UNFREEZE_DELAY_DAYS` (boolean>0): used in VM-native; actuator validate only checks new resource model; include for completeness in helpers.
- `ACCOUNT_UPGRADE_COST`, `ALLOW_MULTI_SIGN`, `ALLOW_BLACKHOLE_OPTIMIZATION` (reference patterns exist for witness create; reuse patterns).

Tron Power calculation (Phase 1 → 2):
- Phase 1: compute owner’s `tronPower` from freeze ledger TRON_POWER (resource=2) if present; otherwise 0 → likely triggers validation failure unless test seeds ledger. Document requirement for tests.
- Phase 2: parse owner Account protobuf and compute exactly:
  - `getTronPower()` (legacy field) and `getAllTronPower()` per `AccountCapsule` logic including V1/V2 freezes and oldTronPower semantics.

---

## Parsing Plan (VoteWitnessContract)

Payload format (`VoteWitnessContract`):
- `owner_address` (bytes, field=1) — treat as informational; canonical owner is `transaction.from`.
- `repeated Vote` (field=2);
  - `Vote.vote_address` (bytes, field=1)
  - `Vote.vote_count` (int64, field=2)
- `support` (bool, field=3) — not used.

Implementation:
- Add a compact parser using the existing varint helpers (`service.rs: read_varint`), sufficient to extract `votes: Vec<(Address, u64)>` and ignore `support`.
- Address canonicalization: expect 21-byte Tron addresses from Java; strip 0x41 into 20-byte EVM `Address` for internal queries.

---

## Execution Semantics & State Changes

- Bandwidth: reuse `calculate_bandwidth_usage(transaction)`.
- Energy: 0 (system NonVM).
- State changes:
  - Exactly one `AccountChange` for owner with `old_account` == `new_account` (no balance/nonce/code delta), for CSV parity.
  - Do not emit raw votes storage deltas by default.

Feature flags:
- Must check `execution.remote.system_enabled` and `execution.remote.vote_witness_enabled`.
- Optional `execution.remote.emit_storage_changes` future switch for emitting witness/vote records in `state_changes` (default false).

---

## TODOs — Phase 1 (MVP, actuator-accurate core)

[ ] Core dispatch (already present): confirm `execute_non_vm_contract()` branches to `execute_vote_witness_contract()` when `contract_type=VoteWitnessContract`.
[ ] Implement `execute_vote_witness_contract()` in `rust-backend/crates/core/src/service.rs`:
  - [ ] Parse `VoteWitnessContract` bytes from `transaction.data` into `(votes: Vec<(Address, u64)>, owner=transaction.from)`.
  - [ ] Validate:
    - [ ] `votes.len() > 0 && votes.len() <= 30`.
    - [ ] Each `vote_count > 0`.
    - [ ] Each `vote_address` is a valid Tron address (21 bytes, 0x41 prefix) and exists as an account (`get_account`) and as a witness (`get_witness`).
    - [ ] Sum TRX vote counts, multiply by `TRX_PRECISION=1_000_000` using checked math.
    - [ ] `sum_sun <= tron_power_sun(owner)`, using resource model flag.
  - [ ] Execution:
    - [ ] (Phase 1) Log skip for `withdrawReward` to avoid delegation dependency.
    - [ ] Create/update `VotesRecord` for owner: set `old_votes` to previous `new_votes` (or empty if none), `clear_new_votes`, append new votes, and persist to `votes` DB.
    - [ ] (Optional in P1) Do not mutate `Account` votes list (defer to Phase 2 for full parity).
  - [ ] Result: success, `energy_used=0`, `bandwidth_used`, one `AccountChange` (owner old==new), no logs.

[ ] Add `VotesRecord` helpers in `rust-backend/crates/execution/src/storage_adapter.rs`:
  - [ ] `votes_database() -> &str` returns "votes".
  - [ ] `votes_key(&Address) -> Vec<u8>` returns 21-byte Tron address key.
  - [ ] `get_votes(&Address) -> Result<Option<VotesRecord>>`.
  - [ ] `set_votes(Address, &VotesRecord) -> Result<()>`.
  - [ ] `VotesRecord` struct with `address: Address`, `old_votes: Vec<(Address,u64)>`, `new_votes: Vec<(Address,u64)>` and exact protobuf serialize/deserialize.

[ ] Dynamic properties helpers (read-only):
  - [ ] `support_allow_new_resource_model() -> Result<bool>` (key: `ALLOW_NEW_RESOURCE_MODEL`).
  - [ ] (optional) `support_unfreeze_delay() -> Result<bool>` (key: `UNFREEZE_DELAY_DAYS` > 0) for completeness.

[ ] Tron power computation (minimum viable for tests):
  - [ ] `get_tron_power_in_sun(address, new_model: bool) -> Result<u64>` using freeze ledger TRON_POWER (resource=2) via `get_freeze_record()`; default to 0 when absent.

[ ] Tests (Rust): add to `rust-backend/crates/core/src/tests.rs`:
  - [ ] `test_vote_witness_success_basic` (seed witness entries and freeze ledger; expect success, 0 energy, 1 account change, votes persisted).
  - [ ] `test_vote_witness_exceeds_max_votes` (31 votes → error message).
  - [ ] `test_vote_witness_invalid_vote_count_zero` (vote_count=0 → error message).
  - [ ] `test_vote_witness_missing_witness` (non-existent witness → error message).
  - [ ] `test_vote_witness_over_power` (sum exceeds tronPower → error message).

[ ] Config & flags:
  - [ ] Ensure `execution.remote.vote_witness_enabled` is read (default false in `common/config.rs`, overridden by `rust-backend/config.toml`).
  - [ ] Keep `emit_storage_changes=false` to avoid CSV drift.

Deliverable acceptance for Phase 1:
- Energy 0, 1 deterministic account change, votes DB updated, actuator-accurate validation and messages, flag-gated.

---

## TODOs — Phase 2 (Full parity)

[ ] Withdraw reward before applying votes (mirrors `MortgageService.withdrawReward`) — requires porting delegation store read path or a parity-safe stub:
  - [ ] Minimal: no-op but log; Advanced: update owner allowance by reading delegation cycles & rewards.

[ ] Account votes list parity:
  - [ ] Read owner Account protobuf from `account` DB; update repeated `votes` list to reflect new votes, and write back (maintaining all other fields).

[ ] Tron power parity:
  - [ ] Parse owner Account protobuf to compute `getTronPower()`/`getAllTronPower()` exactly as per `AccountCapsule`:
    - [ ] If `ALLOW_NEW_RESOURCE_MODEL=1`: use `getAllTronPower()` combining legacy + V1/V2 + oldTronPower semantics.
    - [ ] Else: legacy `getTronPower()`.

[ ] Optional: Emit storage deltas for votes (behind `emit_storage_changes=true`) for richer state replay.

[ ] Java integration tests (optional): enable remote mode and run actuator tests where feasible (storage SPI present):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.VoteWitnessActuatorTest"` with backend running, `STORAGE_MODE=remote`.

---

## Implementation Checklist by File

`rust-backend/crates/core/src/service.rs`
- [ ] Add `parse_vote_witness_contract(&[u8]) -> Result<Vec<(Address,u64)>>`.
- [ ] Implement `execute_vote_witness_contract(...)`:
  - Parse, validate, compute tron power, update votes, build result.
  - Use existing `calculate_bandwidth_usage`.
  - Match error messages with actuator.

`rust-backend/crates/execution/src/storage_adapter.rs`
- [ ] Add votes DB helpers (`votes_database`, `votes_key`).
- [ ] Implement `VotesRecord` + (de)serialization for `protocol.Votes` and `protocol.Vote`.
- [ ] Implement `get_votes`, `set_votes`.
- [ ] Add dynamic property getters: `support_allow_new_resource_model`, `support_unfreeze_delay`.
- [ ] Add tron power helper: `get_tron_power_in_sun(...)` reading TRON_POWER freeze ledger.

`rust-backend/crates/common/src/config.rs`
- [ ] Confirm `RemoteExecutionConfig.vote_witness_enabled` default is false; keep config.toml override if needed.

`rust-backend/config.toml`
- [ ] Ensure `execution.remote.vote_witness_enabled = true` for local dev; keep defaults conservative in code.

---

## Error Messages (Parity Targets)

- `"VoteNumber must more than 0"`
- `"VoteNumber more than maxVoteNumber 30"`
- `"Invalid vote address!"` (invalid or malformed 21-byte address)
- `"account <base58(addr)> not exist"`
- `"Witness <base58(addr)> not exist"`
- `"vote count must be greater than 0"`
- `"The total number of votes[<sum>] is greater than the tronPower[<tp>]"`

Use the same casing and wording to maximize test compatibility.

---

## Risks & Mitigations

- Tron power derivation (Phase 1):
  - Risk: underestimation without parsing full Account; may fail validation in some historical blocks.
  - Mitigation: seed freeze ledger in tests; promote Phase 2 parity work.

- Withdraw reward semantics:
  - Risk: allowance/accounting divergence from embedded during the block.
  - Mitigation: keep as Phase 2; log explicit skip to aid comparison.

- Duplicate votes for same witness:
  - Actuator path does not merge; VM-native merges. Following Actuator avoids surprises. Document and keep tests accordingly.

- CSV parity:
  - Risk: emitting storage deltas could drift CSVs.
  - Mitigation: default off; only enable behind `emit_storage_changes`.

---

## Testing Matrix

Unit (Rust):
- Positive path: 1–N votes, within power, existing witnesses.
- Negative cases: 0 votes, >30 votes, zero/negative counts, missing witness, invalid address length/prefix, sum > power.

Integration (optional, later):
- Java remote execution + SPI writes reflected in local reads (VotesStore fetch in Java after execution should see new votes).

---

## Rollout

1) Land Phase 1 behind `vote_witness_enabled` (off by default in code; opt-in via config.toml in dev).
2) Run focused tests; verify CSV parity (1 account change, 0 energy) and votes DB persistence.
3) Phase 2 for reward + Account parity + power parity.
4) Consider enabling by default after sustained parity runs.

---

## Debugging Tips

- Use tracing logs around: parsed votes, sum (TRX and SUN), computed tronPower, validation failures.
- Log owner + base58 witnesses for any missing entries.
- Enable adapter I/O logs for `votes` DB put/get and `witness`/`account` existence checks.

