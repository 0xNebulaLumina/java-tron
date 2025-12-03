• Here’s a focused, end‑to‑end plan to diagnose, fix, and validate the witness-schedule mismatch you hit in remote execution + remote storage.

Diagnosis Summary

- Embedded path seeds old_votes from the owner’s prior votes persisted in the Account before writing a VotesStore record:
    - actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java:171–176
- Remote path creates an empty old_votes on first write (when no VotesRecord exists), so decrements never happen for changed votes:
    - rust-backend/crates/core/src/service/mod.rs:1143–1153, 1160–1166
- Maintenance computes delta = new_votes − old_votes to update witness tallies:
    - consensus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:162–191
- In your logs, maintenance runs and updates witnesses, then the next block fails witness validation:
    - “There is 17 new votes…” and “Update witness success” followed by “validateWitnessSchedule error”
    - remote-java.e7cd34a.log:208478, 208497, 209320

Goal

- Make the remote path seed old_votes the same way embedded does (from the Account’s current votes list) on first write; keep subsequent writes using “old = previous new” behavior. This restores correct vote
deltas, witness tallies, witness order, and block validation.

Plan

- Confirm and Instrument
    - Add targeted debug to verify delta math at maintenance time: print per-witness delta and the size of the VotesStore iterator.
    - Capture the witness list before/after maintenance with tallies to visualize divergence versus embedded:
        - consensus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:146–148
    - Rust logs already show first-write behavior (“No existing votes… creating new record”) and old/new lengths; keep these for correlation.
- Implement Rust seeding of old_votes
    - Add a method to read the owner’s existing votes from the Account record (field 5 = repeated Vote):
        - File: rust-backend/crates/execution/src/storage_adapter/engine.rs
        - New method: get_account_votes_list(&self, address)
            - Read raw bytes from account DB using self.account_database() and account key (0x41 + 20-byte address).
            - Parse Account protobuf field 5 (repeated Protocol.Vote) with the same safe parser pattern already used for parsing votes in VoteWitnessContract (service/mod.rs:894–1010).
            - Return Vec<(Address, u64)> for prior votes.
    - Use this method when no VotesRecord exists
        - File: rust-backend/crates/core/src/service/mod.rs
        - In execute_vote_witness_contract:
            - If get_votes(owner) returns None:
                - Read prior_votes = get_account_votes_list(owner).
                - Set votes_record = VotesRecord::new(owner, prior_votes, Vec::new()).
            - Else: keep existing logic (old_votes = previous new_votes).
    - Add a feature gate for safety and staged rollout
        - Config: rust-backend/config.toml → [remote] vote_witness_seed_old_from_account = true (default true).
        - Guard the branch with this flag.
- Optional Java safety net (short‑term fallback)
    - If an urgent hotfix is needed before deploying the Rust change, add a temporary check in maintenance to avoid a bad delta when oldVotes is empty but Account has historical votes:
        - consensus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:162–191
        - If votes.getOldVotes().isEmpty() and account.votes is non-empty and votes.getNewVotes() is non-empty, treat account.votes as oldVotes for delta computation. Delete this once Rust fix is live.
- Tests
    - Rust unit tests
        - Add tests for get_account_votes_list covering:
            - No votes in Account.
            - Multiple votes in Account.
            - Malformed data handling.
    - Rust service tests
        - Seed DB with Account containing non-empty votes; ensure get_votes(owner)==None; execute VoteWitness; assert get_votes(owner).old_votes equals Account votes and that new_votes is the submitted
        contract list.
    - Sanity replay checkpoint
        - Sync to the same epoch boundary (just before your failure at block 4972), run maintenance with seeding enabled, and confirm witness changes match embedded and no validateWitnessSchedule error
        appears.
- Observability and verification
    - Rust logs: when seeding occurs, log “Seeding old_votes from Account votes: N entries” including owner address.
    - Java logs: extend maintenance logging to include top-N witness addresses with vote counts before/after for the next few epochs (then revert).
    - Add a metric counter vote_witness_seeded_from_account in Rust to quantify first-time seeding events.
- Rollout and fallback
    - Roll out the Rust change with the config gate ON.
    - If any unforeseen issue occurs, toggle [remote] vote_witness_seed_old_from_account = false to immediately revert to current behavior while investigating.
- Data considerations
    - Existing bad VotesRecords (with empty old_votes) only affect the first maintenance after creation; post-fix, future votes by the same account will have correct old/new linkage in the VotesRecord.
    - If a node is already stuck on validateWitnessSchedule error, a resync from a safe checkpoint is recommended after the fix; alternatively, purge bad votes entries with caution (only if you know the epoch
    boundaries).

Why this plan resolves your error

- It makes remote behavior match embedded for the very first VoteWitness by an account after an epoch: old_votes is seeded from the Account’s prior votes rather than left empty. Maintenance now sees accurate
deltas (subtract old, add new), producing the correct witness order. That aligns Java’s locally computed schedule with the network, eliminating the validateWitnessSchedule error.

Key Code Touchpoints

- Seed old_votes on first record
    - rust-backend/crates/core/src/service/mod.rs:1143–1153, 1160–1166
- Read Account votes list
    - rust-backend/crates/execution/src/storage_adapter/engine.rs: account DB helper (add get_account_votes_list)
    - Reuse Vote parsing pattern used in service/mod.rs:894–1010
- Maintenance delta application (for optional Java fallback only)
    - consensus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:162–191

