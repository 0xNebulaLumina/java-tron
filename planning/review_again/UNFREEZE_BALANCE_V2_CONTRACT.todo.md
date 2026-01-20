# TODO / Fix plan: `UNFREEZE_BALANCE_V2_CONTRACT` (Rust vs Java parity)

## Goal
Make Rust `UNFREEZE_BALANCE_V2_CONTRACT` behavior match java-tron exactly for:
- persisted state (`account`, `dynamic-properties`, `votes`, delegation stores/allowance),
- validation failure ordering + messages (where fixtures care),
- and remote reporting outputs (freeze ledger change / CSV/domain parity).

## Acceptance criteria
- [ ] Rust conformance fixtures under `conformance/fixtures/unfreeze_balance_v2_contract/*` pass (state diffs + result.pb).
- [ ] For a vote-bearing account under `ALLOW_NEW_RESOURCE_MODEL=1`:
  - [ ] First call that initializes `oldTronPower` clears votes exactly like Java.
  - [ ] Subsequent BANDWIDTH/ENERGY unfreeze calls **do not touch votes** when `oldTronPower == -1`.
- [ ] `MortgageService.withdrawReward(owner)` side effects are replicated when delegation is enabled (allowance + delegation cycle writes match).
- [ ] Any emitted `FreezeLedgerChange` with `v2_model=true` uses `expiration_ms == 0` (V2 has no expiration), including in unfreeze V2.

## Fix checklist

### 1) Align vote early-return under new resource model
- [ ] In `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_unfreeze_balance_v2_contract`):
  - [ ] When `allow_new_resource_model == true` and `new_owner_proto.old_tron_power == -1`:
    - [ ] If resource is `Bandwidth` or `Energy`, **return early from vote update logic** (skip rescale and skip votes-store writes), matching Java `updateVote()`’s early `return`.
    - [ ] If resource is `TronPower`, keep the existing behavior (possible rescaling).
- [ ] Add a regression test for this scenario (see section 4).

### 2) Add `withdrawReward` parity (delegation rewards / allowance)
- [ ] Confirm intended gating rules:
  - [ ] Java: only runs when `DynamicPropertiesStore.allowChangeDelegation()` is enabled.
  - [ ] Rust: decide whether to mirror the dynamic-property gate, and optionally also gate behind config (e.g., `delegation_reward_enabled`).
- [ ] In `execute_unfreeze_balance_v2_contract`, call the Rust port:
  - [ ] `rust-backend/crates/core/src/service/contracts/delegation.rs::withdraw_reward`
  - [ ] Place it in the same order as Java: before sweeping expired `unfrozen_v2` and before resource/vote updates.
- [ ] Verify it produces the same writes as Java in:
  - [ ] delegation cycle keys (begin/end cycle),
  - [ ] allowance updates (account allowance field),
  - [ ] any vote snapshot persistence if applicable.

### 3) Fix V2 “no expiration” semantics in freeze-ledger reporting
Scope note: This touches both `FREEZE_BALANCE_V2_CONTRACT` and `UNFREEZE_BALANCE_V2_CONTRACT`, but unfreeze V2 currently preserves a non-zero expiration if one exists.

- [ ] Decide the canonical rule: for `v2_model=true`, `expiration_ms` must always be `0`.
- [ ] In `execute_unfreeze_balance_v2_contract`:
  - [ ] Stop copying/preserving `existing_expiration` into the updated V2 freeze record; force to `0` (or ignore the field entirely).
  - [ ] Ensure emitted `FreezeLedgerChange.expiration_ms == 0` whenever `v2_model=true` (both partial and full unfreeze).
- [ ] If Rust keeps the `freeze-records` DB:
  - [ ] Ensure V2 paths never set non-zero expirations in that DB.
  - [ ] Consider splitting storage keys by (resource, v2_model) to avoid mixing v1/v2 semantics (optional but safer).

### 4) Tests to add/repair
Rust unit tests currently don’t reflect production inputs well for this contract (they often omit `from_raw` and required dynamic properties).

- [ ] Update/add tests in `rust-backend/crates/core/src/service/tests/contracts.rs` (or a more targeted module):
  - [ ] Provide `TxMetadata.from_raw` with a valid 21-byte prefixed address matching the configured prefix.
  - [ ] Seed `UNFREEZE_DELAY_DAYS` in dynamic-properties so `support_unfreeze_delay()` passes.
  - [ ] Add test: `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=-1`, resource=BANDWIDTH/ENERGY, existing votes → votes unchanged after unfreeze.
  - [ ] Add test: legacy model (`ALLOW_NEW_RESOURCE_MODEL=0`) rescale votes when owned tron power becomes insufficient (this should already exist in fixtures).
  - [ ] Add test: delegation enabled → `withdraw_reward` is invoked and allowance/cycle keys change as expected.

### 5) Validation/error ordering edge cases (optional)
- [ ] Decide whether empty `transaction.data` should:
  - [ ] behave like Java (decode defaults → fail at address validation), or
  - [ ] remain a Rust-only early error.
- [ ] If aligning: remove the `data.is_empty()` hard error in `parse_unfreeze_balance_v2_params` and rely on later validation.

### 6) Verification commands
- [ ] Rust conformance runner (focused):
  - [ ] `cd rust-backend && cargo test -p tron-backend-core conformance::runner::tests::test_run_real_fixtures -- --ignored`
    - (Or run a small harness that executes only `unfreeze_balance_v2_contract` fixtures.)
- [ ] Targeted Rust tests:
  - [ ] `cd rust-backend && cargo test -p tron-backend-core unfreeze_balance_v2`
- [ ] Java sanity (fixture regeneration when needed):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV2FixtureGeneratorTest" --dependency-verification=off`

