# TODO / Fix Plan: `VOTE_WITNESS_CONTRACT` parity

## Goal

Make Rust `VOTE_WITNESS_CONTRACT` match Java `VoteWitnessActuator` semantics for:
- validation + error ordering/messages (to the extent conformance requires)
- vote bookkeeping (`VotesStore` old/new votes + `Account.votes` replacement)
- **execute()-time side effects** that Java performs inside VoteWitness:
  - `MortgageService.withdrawReward(ownerAddress)` (delegation rewards → `Account.allowance` + delegation-store cycle state)
  - `Account.oldTronPower` initialization when the new resource model is enabled

Primary Java oracles to match:
- `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java` (`validate()`, `countVoteAccount()`)
- `chainbase/src/main/java/org/tron/core/service/MortgageService.java` (`withdrawReward`, `adjustAllowance`)
- `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java` (`initializeOldTronPower`)
- `chainbase/src/main/java/org/tron/core/capsule/VotesCapsule.java`

Rust entrypoint to fix:
- `rust-backend/crates/core/src/service/mod.rs` → `BackendService::execute_vote_witness_contract(...)`

---

## Checklist (tactical)

- [ ] Confirm parity scope + write model assumptions
  - [ ] Decide whether VoteWitness must work in both:
    - [ ] `WRITE_MODE_PERSISTED` (Rust persists; Java skips apply)
    - [ ] `WRITE_MODE_COMPUTE_ONLY` (Java applies changes)
  - [ ] If compute-only is required, decide how to represent TRON-proto side effects (allowance, delegation-store updates, old_tron_power):
    - [ ] Extend `backend.proto` with explicit sidecars (recommended), or
    - [ ] implement generic “storage write diff” emission (if that’s the intended role of `emit_storage_changes`), or
    - [ ] temporarily force VoteWitness to only run in persisted mode and fall back to Java otherwise.

- [ ] Implement `MortgageService.withdrawReward(ownerAddress)` semantics for VoteWitness (major)
  - [ ] Match Java ordering: call withdrawReward **before** mutating votes (Java does it at the start of `countVoteAccount()`).
  - [ ] Rust touchpoints:
    - [ ] `rust-backend/crates/core/src/service/mod.rs` (`execute_vote_witness_contract`)
    - [ ] `rust-backend/crates/core/src/service/contracts/delegation.rs` (`withdraw_reward`)
  - [ ] Implementation approach:
    - [ ] Call `delegation::withdraw_reward(storage_adapter, &owner)` (it already no-ops when `allowChangeDelegation == false`).
    - [ ] If returned `reward > 0`, apply Java’s `adjustAllowance(owner, reward)` effect:
      - [ ] load owner `protocol::Account` proto
      - [ ] `account.allowance += reward` (keep Java semantics; decide whether to add overflow checks)
      - [ ] persist updated owner account proto
    - [ ] Ensure delegation-store begin/end cycle state and accountVote snapshots are updated exactly once (the Rust port already writes these).
  - [ ] Verification:
    - [ ] Add a regression test where `allowChangeDelegation == true` and rewards exist across cycles:
      - [ ] VoteWitness increases `Account.allowance` by the expected amount
      - [ ] delegation-store cycle keys (`beginCycle`, `endCycle`, `accountVote`) match Java expectations
    - [ ] Confirm no behavior change when `allowChangeDelegation == false` (should be a no-op).

- [ ] Implement `oldTronPower` initialization for VoteWitness under the new resource model (major-ish)
  - [ ] Java behavior:
    - [ ] if `ALLOW_NEW_RESOURCE_MODEL == 1` and `account.oldTronPower == 0`:
      - [ ] set `oldTronPower = getTronPower()`; if that is `0`, set `-1`
  - [ ] Rust touchpoints:
    - [ ] `rust-backend/crates/core/src/service/mod.rs` (`execute_vote_witness_contract`)
    - [ ] `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`compute_tron_power_in_sun` already exists)
  - [ ] Implementation approach:
    - [ ] after loading owner account proto (and before persisting it), if `support_allow_new_resource_model()` and `old_tron_power == 0`:
      - [ ] compute `tron_power = compute_tron_power_in_sun(owner, new_model=false)` (Java `getTronPower()`)
      - [ ] set `old_tron_power = (tron_power == 0 ? -1 : tron_power as i64)`
  - [ ] Tests:
    - [ ] owner with `old_tron_power=0` transitions to `-1` when `getTronPower()==0`
    - [ ] owner with `old_tron_power=0` transitions to positive snapshot when `getTronPower()>0`

- [ ] Keep vote bookkeeping parity stable (don’t regress existing behavior)
  - [ ] Maintain current behavior:
    - [ ] do not mutate `old_votes` during VoteWitness; only replace `new_votes`
    - [ ] when VotesRecord is missing, seed `old_votes` from `Account.votes` (config default true)
  - [ ] Add/extend tests for:
    - [ ] first VoteWitness with non-empty `Account.votes` seeds `old_votes` correctly
    - [ ] second VoteWitness in same epoch does not shift `old_votes`

- [ ] Decide and implement propagation strategy for compute-only mode (if required)
  - [ ] If `WRITE_MODE_COMPUTE_ONLY` must be supported, implement one of:
    - [ ] **New sidecar(s)** for VoteWitness:
      - [ ] `AllowanceChange { owner_address, delta_allowance }` (or absolute allowance)
      - [ ] `DelegationStoreChange` / `DelegationSnapshotChange` (begin/end/accountVote updates), or a compact encoding sufficient for Java apply
      - [ ] `OldTronPowerChange { owner_address, old_tron_power }`
    - [ ] Or implement `emit_storage_changes` as a generic key-value write-diff stream for non-EVM stores (Account proto keys, DelegationStore keys, VotesStore keys).
  - [ ] Update Java apply layer (`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`) to apply whichever representation is chosen.

- [ ] Rollout / safety checklist
  - [ ] Keep `execution.remote.vote_witness_enabled` gated until all above is verified
  - [ ] Add at least one end-to-end parity run where:
    - [ ] `allowChangeDelegation == true`
    - [ ] a VoteWitness happens after rewards have accrued
    - [ ] subsequent `WithdrawBalance` observes the updated allowance

