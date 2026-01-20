# Review: `VOTE_WITNESS_CONTRACT` parity (rust-backend vs java-tron)

## TL;DR

Rust’s `VOTE_WITNESS_CONTRACT` implementation **matches the Java VoteWitnessActuator’s validation and the core vote-record updates** (VotesStore/VotesCapsule `newVotes` + `Account.votes` replacement), including the important “first vote in epoch” behavior (seed `old_votes` from `Account.votes` when the VotesRecord doesn’t exist).

But it **does not fully match Java execution semantics** in two consensus-relevant ways:

1) **Missing `MortgageService.withdrawReward(ownerAddress)` side-effects (major).**
   - Java calls this at the start of `execute()` and it is not a no-op when `allowChangeDelegation == true`.
   - Rust currently logs “Skipping withdrawReward … delegation not yet ported” and performs no equivalent allowance/delegation-store updates.

2) **Missing `oldTronPower` initialization under the new resource model (major-ish).**
   - Java sets `Account.oldTronPower` when `ALLOW_NEW_RESOURCE_MODEL == 1` and `oldTronPower == 0`.
   - Rust does not update `old_tron_power` in VoteWitness execution.

If you enable Rust execution for VoteWitness (as `rust-backend/config.toml` currently does), it’s “close” for votes bookkeeping, but it **is not Java-equivalent state transition logic**.

---

## Rust entrypoint + summary

Rust entrypoint:
- `rust-backend/crates/core/src/service/mod.rs` → `BackendService::execute_vote_witness_contract(...)`

High-level flow (Rust):
1) Parse `VoteWitnessContract` from protobuf bytes in `transaction.data` (custom parser).
2) Validate:
   - `owner_address` + each `vote_address`: length=21 and prefix matches `storage_adapter.address_prefix()`
   - `votes_count`: `> 0` and `<= MAX_VOTE_NUMBER`
   - each `vote_count > 0`
   - witness account exists + witness exists
   - `sum(vote_count) * TRX_PRECISION <= tronPower(owner)` using `support_allow_new_resource_model()` + `compute_tron_power_in_sun(...)`
3) **Skip** delegation reward withdrawal (`withdrawReward`) entirely (log only).
4) Load or create `VotesRecord`:
   - when missing, optionally seed `old_votes` from `Account.votes` (`vote_witness_seed_old_from_account`, default true)
   - clear and replace `new_votes` with the tx’s votes
5) Persist:
   - `VotesRecord` to VotesStore
   - overwrite `Account.votes` list to match the tx’s votes
6) Emit a `VoteChange` sidecar (for compute-only mode) and a placeholder `AccountChange` (old==new) for CSV parity.

---

## Java oracle behavior

Primary Java reference:
- `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java`
  - `validate()` defines error ordering/messages and tronPower check
  - `countVoteAccount()` performs the state mutations

Supporting Java references:
- `chainbase/src/main/java/org/tron/core/service/MortgageService.java` (`withdrawReward`, `adjustAllowance`)
- `chainbase/src/main/java/org/tron/core/capsule/VotesCapsule.java` (oldVotes/newVotes storage shape)
- `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java` (`initializeOldTronPower`, votes list)

Java `countVoteAccount()` semantics (simplified):
1) `mortgageService.withdrawReward(ownerAddress);`
2) If `ALLOW_NEW_RESOURCE_MODEL == 1` and `oldTronPower == 0`: `initializeOldTronPower()`
3) If VotesStore has no entry: `new VotesCapsule(owner, account.getVotesList())` (seed `oldVotes`)
4) Clear `account.votes` and `votesCapsule.newVotes`, then add tx votes to both
5) Persist Account + VotesCapsule

---

## Where Rust matches Java (important)

- **Validation parity (VoteWitnessActuator.validate)**
  - owner/vote address length + prefix checks (`Invalid address`, `Invalid vote address!`)
  - `votes_count` bounds (`VoteNumber must more than 0`, `VoteNumber more than maxVoteNumber 30`)
  - per-vote witness existence checks (`Account[...] not exists`, `Witness[...] not exists`)
  - `vote_count > 0`
  - tronPower check:
    - uses `ALLOW_NEW_RESOURCE_MODEL` to select `getTronPower()` vs `getAllTronPower()` semantics
    - multiplies vote sum by `TRX_PRECISION` (TRX→SUN) before comparing

- **Vote storage semantics (VoteWitnessActuator.countVoteAccount)**
  - preserves `old_votes` / `oldVotes` as epoch baseline and only overwrites `new_votes` / `newVotes`
  - seeds `old_votes` from `Account.votes` on first VotesRecord creation (when enabled; default true)
  - replaces `Account.votes` with the tx vote list (clear + append)

---

## Concrete mismatches / parity risks

### 1) Missing `MortgageService.withdrawReward(ownerAddress)` (major)

Java executes `withdrawReward` *inside* VoteWitness execution.
That affects:
- `DelegationStore` cycle state (`beginCycle`, `endCycle`) and `accountVote` snapshots
- `Account.allowance` (reward balance available for `WithdrawBalanceContract`)

Rust currently does not call the Rust port of this logic:
- Rust has a port at `rust-backend/crates/core/src/service/contracts/delegation.rs` (`withdraw_reward`)
- VoteWitness execution explicitly skips it

Impact:
- allowance and delegation-store state diverge from Java after VoteWitness whenever `allowChangeDelegation == true`
- reward accounting can become incorrect because the reward is computed using the *old* vote set before the vote change (Java does this intentionally)

### 2) Missing `oldTronPower` initialization in VoteWitness (major-ish)

Java:
- when `supportAllowNewResourceModel()` and `account.oldTronPower == 0`, it sets:
  - `oldTronPower = getTronPower()` (or `-1` when `getTronPower()==0`)

Rust:
- computes tronPower correctly for the validation check, but does not persist the `old_tron_power` field update that Java performs in `execute()`.

Impact:
- state divergence on `Account.old_tron_power`, which can affect later `getAllTronPower()` behavior and maintenance semantics.

### 3) Compute-only vs persisted write model complication (design mismatch)

VoteWitness has TRON-proto side effects (Account.allowance, delegation-store keys, old_tron_power) that **are not representable** in the current EVM-style `AccountChange` / `AccountInfo` model.

Currently:
- Rust can “get away with it” because it **doesn’t perform those side effects**.
- If you implement them, you must decide how Java receives/applies them when `write_mode == COMPUTE_ONLY`:
  - there is a sidecar for `VoteChange`, but not for allowance increments or delegation-store mutations.

If you only support `WRITE_MODE_PERSISTED` for VoteWitness, then Rust can persist these effects directly; but that weakens the “Option A” design goal (Java applies everything).

### 4) Minor edge-cases / consistency notes

- Rust VoteWitness does not currently do an `Any.is(...)`/`type_url` check against `transaction.metadata.contract_parameter` (some other contracts do).
- gRPC response address prefixing (`add_tron_address_prefix`) hardcodes `0x41` (not `storage_adapter.address_prefix()`), which can be a parity issue for non-0x41 networks/fixtures.

---

## Conclusion

Rust’s VoteWitness implementation is **not fully Java-equivalent** today.
It matches the *vote list / VotesStore bookkeeping*, but **it is missing delegation reward withdrawal and oldTronPower initialization**, both of which are part of Java’s `VoteWitnessActuator.execute()` state transition.

