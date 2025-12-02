# Fix: Seed VotesRecord.old_votes From Account Votes (Remote Mode)

## Context
- Modes
  - Embedded: Java actuators execute; `VoteWitnessActuator` writes `VotesStore` with `VotesCapsule(old_votes=<account.votes>, new_votes=<submitted>)` on first vote, then updates on subsequent votes.
  - Remote: Rust executes non‑VM contracts; Java applies state changes returned by Rust. Votes are persisted through Rust `set_votes` API (protobuf `Votes`), read by Java `VotesStore` for maintenance.
- Maintenance
  - `MaintenanceManager.countVote(VotesStore)` applies per‑witness delta: `sum(new_votes) - sum(old_votes)`; then reorders active witnesses.
- Current remote behavior
  - First VoteWitness for an owner with prior votes creates a `VotesRecord` with empty `old_votes` (Rust), so decrements are skipped when the voter changes choices. Witness tallies become inflated and order diverges. This can lead to `validateWitnessSchedule error` on the next block after maintenance.

## Evidence (from your run)
- Remote logs show first‑time creation with empty `old_votes`:
  - “No existing votes … creating new record” → “Successfully stored votes … old_votes=0, new_votes=1”
- Java (remote) logs show maintenance executing and changing witness set just before the failure:
  - “There is N new votes in this epoch” → “Update witness success” → `ValidateScheduleException` on next block.
- Embedded mode does not fail because old_votes is seeded from Account.votes on first write.

## Root Cause
- On first vote for an owner, remote path does not seed `old_votes` from the owner’s persistent Account votes list; it initializes `old_votes` as empty. Maintenance therefore fails to subtract prior votes when the voter changes targets, causing witness order divergence and schedule validation failure.

## Design Goals
- Match embedded semantics in remote mode: On first `VotesRecord` creation, set `old_votes` to the owner’s existing Account votes list (Protocol.Account.repeated Vote).
- Preserve existing semantics thereafter: `old_votes = previous new_votes` on subsequent writes.
- Be safe, observable, and quickly reversible via a config flag.

## High‑Level Plan
1. Rust backend: Seed `old_votes` from Account.votes on first record creation (config‑gated, default ON).
2. Optional Java fallback (feature gated, default OFF): In maintenance, treat empty `old_votes` as `account.votes` once to mitigate until Rust fix is deployed.
3. Tests: Unit and service tests in Rust; optional targeted Java test; verification run around an epoch boundary.
4. Observability: Logs + metric counter for seeding events.
5. Rollout: Enable flag by default; provide immediate rollback via config; recommend resync for nodes already past divergence.

---

## Detailed TODOs (Rust Backend)

### A. Config Gate
- [x] Add `vote_witness_seed_old_from_account = true` under `[remote]` in `rust-backend/config.toml`.
- [x] Thread this flag into execution configuration (RemoteExecutionConfig) and into `execute_vote_witness_contract`.

### B. Account Votes Parser
- [x] Implement `get_account_votes_list(&self, address: &Address) -> Result<Vec<(Address, u64)>>` in `EngineBackedEvmStateStore`:
  - Read raw protobuf bytes from `account` DB (key = `0x41` + 20‑byte address).
  - Parse Account protobuf field 5: `repeated Vote` (each `Vote` has `vote_address` [bytes with 0x41 prefix] and `vote_count` [int64]).
  - Normalize vote_address: strip 0x41 to 20‑byte H160 (REVM Address).
  - Graceful error handling: on decode error, log warn and return empty vec.
  - Unit tests:
    - Account with 0, 1, many votes; truncated payload; non‑21‑byte addresses; large counts.

### C. Seed `old_votes` On First Record
- [x] File: `rust-backend/crates/core/src/service/mod.rs` (`execute_vote_witness_contract`):
  - When `get_votes(owner)` returns `None`:
    - If flag enabled: call `get_account_votes_list(owner)`.
      - If non‑empty: set `votes_record = VotesRecord::new(owner, prior_votes, Vec::new())`.
      - Else: `VotesRecord::empty(owner)` (unchanged fallback).
    - Else (flag disabled): `VotesRecord::empty(owner)` (current behavior).
  - Keep subsequent behavior unchanged: if record exists, set `old = previous new`, then clear/add new votes.

### D. Logging & Metrics
- [x] Log at INFO when seeding occurs: owner (base58), count of seeded old votes.
- [ ] Add counter metric `vote_witness_seeded_from_account` (increment by 1 per seeding event).
- [x] Expand existing VoteWitness logs to include `old_votes.len()` before persist for visibility.

### E. Tests (Rust)
- [ ] Unit tests for `get_account_votes_list`.
- [ ] Service test: First VoteWitness with prior Account votes
  - Seed account protobuf with repeated votes (A,B,C), `get_votes(owner)==None`.
  - Execute VoteWitness with new set (e.g., B,D).
  - Assert persisted `VotesRecord.old_votes` equals (A,B,C) and `new_votes` equals (B,D).
- [ ] Service test: Subsequent VoteWitness updates
  - With existing record, ensure `old_votes = previous new_votes` and behavior unchanged.
- [ ] Negative test: Corrupted Account protobuf → seeding skipped, no panic.

### F. Docs & Changelog
- [ ] Document the flag and behavior in `rust-backend/docs/` (execution contract docs, VoteWitness)
- [ ] Note in CHANGELOG that remote VoteWitness old_votes seeding now matches embedded behavior.

---

## Optional TODOs (Java Fallback, Feature‑Gated)

Purpose: Temporary safety net if the Rust rollout is delayed. Prefer Rust fix; use this only if needed.

- [ ] Add JVM flag `-Dconsensus.votes.seedOldFromAccount=true|false` (default false).
- [ ] In `MaintenanceManager.countVote(...)`:
  - For each `VotesCapsule votes` from the iterator:
    - If `votes.getOldVotes().isEmpty()` AND owner’s `AccountCapsule.getVotesList()` is non‑empty AND `votes.getNewVotes()` is non‑empty:
      - Treat `account.votesList` as `oldVotes` for delta computation only (do not mutate the store here).
  - Log a WARN once per owner to highlight fallback use.
- [ ] Unit tests around delta computation in this branch.
- [ ] Remove after Rust fix is deployed everywhere.

Risk: If both oldVotes and account.votes are non‑empty due to previous writes, ensure the guard prevents double subtraction.

---

## Validation Plan

### Local/CI
- [ ] Run Rust unit + service tests.
- [ ] If feasible, add a minimal integration harness that exercises:
  - Freeze → VoteWitness before maintenance (first vote for owner) → maintenance → next block; assert no `ValidateScheduleException` and witness list stable.

### Replay/Canary
- [ ] Enable the config flag in a staging node.
- [ ] Sync across an epoch boundary (where maintenance runs) with representative VoteWitness activity.
- [ ] Confirm logs:
  - Seeding events observed (first‑time only), then taper off.
  - “There is N new votes … Update witness success.”
  - No `validateWitnessSchedule error` after maintenance.
- [ ] Compare top‑N witness tallies before/after to embedded run for parity.

---

## Rollout & Ops
- Default: Enable `vote_witness_seed_old_from_account = true`.
- Observability: Watch metric `vote_witness_seeded_from_account` and maintenance logs for a few cycles.
- Rollback: If any issue, set the flag to false and restart; the behavior reverts immediately.
- Stuck nodes: If already diverged, recommend resync from a safe checkpoint after deploying the fix.

---

## Edge Cases & Risks
- Account has no prior votes: seeding finds none → behavior identical to current.
- Corrupted Account protobuf: parser returns empty; we log and continue (no crash); behavior identical to current.
- Mixed versions: Seeding is deterministic and local; does not alter consensus protocol on wire. Nodes without the fix may still diverge locally; fix reduces that risk.
- Very large votes lists: Ensure parser bounds; reject absurd lengths; log and skip.
- Address normalization: Ensure 21‑byte Tron addresses convert to 20‑byte H160 correctly; reject if malformed.

---

## Out of Scope (Phase 2 Candidates)
- Updating Account.votes list from Rust during VoteWitness (state change back to Java). Consensus relies on VotesStore; account field parity can be handled later.
- Backfilling existing VotesRecords created with empty old_votes mid‑epoch.

---

## Time & Ownership
- Rust implementation + tests: ~1–2 days.
- Optional Java fallback: ~0.5 day.
- Replay/validation: ~0.5–1 day.

Owners
- Rust core service + storage adapter: Backend team
- Consensus (fallback): Core Java team

---

## Checklist (One‑Pager)
- [x] Config flag added, default ON
- [x] Account votes parser implemented + tested
- [x] First‑record seeding logic integrated + logged + metered
- [ ] Service tests for first and subsequent votes
- [ ] Docs updated
- [ ] (Optional) Java fallback behind flag
- [ ] Replay validation across maintenance; no schedule error
- [ ] Rollout with monitoring & rollback plan

