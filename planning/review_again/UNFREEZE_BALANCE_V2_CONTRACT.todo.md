# TODO / Fix plan: `UNFREEZE_BALANCE_V2_CONTRACT` (Rust vs Java parity)

## Goal
Make Rust `UNFREEZE_BALANCE_V2_CONTRACT` behavior match java-tron exactly for:
- persisted state (`account`, `dynamic-properties`, `votes`, delegation stores/allowance),
- validation failure ordering + messages (where fixtures care),
- and remote reporting outputs (freeze ledger change / CSV/domain parity).

## Acceptance criteria
- [x] Rust conformance fixtures under `conformance/fixtures/unfreeze_balance_v2_contract/*` pass (state diffs + result.pb).
- [x] For a vote-bearing account under `ALLOW_NEW_RESOURCE_MODEL=1`:
  - [x] First call that initializes `oldTronPower` clears votes exactly like Java.
  - [x] Subsequent BANDWIDTH/ENERGY unfreeze calls **do not touch votes** when `oldTronPower == -1`.
- [x] `MortgageService.withdrawReward(owner)` side effects are replicated when delegation is enabled (allowance + delegation cycle writes match).
- [x] Any emitted `FreezeLedgerChange` with `v2_model=true` uses `expiration_ms == 0` (V2 has no expiration), including in unfreeze V2.

## Fix checklist

### 1) Align vote early-return under new resource model
- [x] In `rust-backend/crates/core/src/service/contracts/freeze.rs` (`execute_unfreeze_balance_v2_contract`):
  - [x] When `allow_new_resource_model == true` and `new_owner_proto.old_tron_power == -1`:
    - [x] If resource is `Bandwidth` or `Energy`, **return early from vote update logic** (skip rescale and skip votes-store writes), matching Java `updateVote()`'s early `return`.
    - [x] If resource is `TronPower`, keep the existing behavior (possible rescaling).
- [x] Add a regression test for this scenario (see section 4).

### 2) Add `withdrawReward` parity (delegation rewards / allowance)
- [x] Confirm intended gating rules:
  - [x] Java: only runs when `DynamicPropertiesStore.allowChangeDelegation()` is enabled.
  - [x] Rust: mirrors dynamic-property gate via `allow_change_delegation()` inside `withdraw_reward`, and also gates behind config (`delegation_reward_enabled`) via `compute_delegation_reward_if_enabled`.
- [x] In `execute_unfreeze_balance_v2_contract`, call the Rust port:
  - [x] `rust-backend/crates/core/src/service/contracts/delegation.rs::withdraw_reward`
  - [x] Place it in the same order as Java: before sweeping expired `unfrozen_v2` and before resource/vote updates.
- [x] Verify it produces the same writes as Java in:
  - [x] delegation cycle keys (begin/end cycle),
  - [x] allowance updates (account allowance field),
  - [x] any vote snapshot persistence if applicable.

### 3) Fix V2 "no expiration" semantics in freeze-ledger reporting
Scope note: This touches both `FREEZE_BALANCE_V2_CONTRACT` and `UNFREEZE_BALANCE_V2_CONTRACT`, but unfreeze V2 currently preserves a non-zero expiration if one exists.

- [x] Decide the canonical rule: for `v2_model=true`, `expiration_ms` must always be `0`.
- [x] In `execute_unfreeze_balance_v2_contract`:
  - [x] Stop copying/preserving `existing_expiration` into the updated V2 freeze record; force to `0` (or ignore the field entirely).
  - [x] Ensure emitted `FreezeLedgerChange.expiration_ms == 0` whenever `v2_model=true` (both partial and full unfreeze).
- [x] If Rust keeps the `freeze-records` DB:
  - [x] Ensure V2 paths never set non-zero expirations in that DB.
  - [ ] Consider splitting storage keys by (resource, v2_model) to avoid mixing v1/v2 semantics (optional but safer).

### 4) Tests to add/repair
Rust unit tests currently don't reflect production inputs well for this contract (they often omit `from_raw` and required dynamic properties).

- [x] Update/add tests in `rust-backend/crates/core/src/service/tests/contracts/freeze_balance.rs`:
  - [x] Provide `TxMetadata.from_raw` with a valid 21-byte prefixed address matching the configured prefix.
  - [x] Seed `UNFREEZE_DELAY_DAYS` in dynamic-properties so `support_unfreeze_delay()` passes.
  - [x] Add test: `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=-1`, resource=BANDWIDTH/ENERGY, existing votes → votes unchanged after unfreeze.
  - [x] Add test: `ALLOW_NEW_RESOURCE_MODEL=1`, `old_tron_power=-1`, resource=TRON_POWER, rescale votes when owned tron power becomes insufficient.
  - [x] Add test: delegation enabled → `withdraw_reward` is invoked and allowance/cycle keys change as expected.
  - [x] Add test: delegation disabled → allowance unchanged.
  - [x] Add test: V2 freeze-ledger expiration is always 0 even when freeze record has non-zero expiration.

### 5) Validation/error ordering edge cases (optional)
- [x] Decide whether empty `transaction.data` should:
  - [x] Remain a Rust-only early error (decided: keep current behavior — this is a minor edge case that doesn't affect real transactions, and the error is still informative).

### 6) Verification commands
- [x] Rust conformance runner (focused):
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only` — all fixtures pass.
- [x] Targeted Rust tests:
  - [x] `cd rust-backend && cargo test -p tron-backend-core unfreeze_v2` — all 23 tests pass (including 6 new).
- [ ] Java sanity (fixture regeneration when needed):
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV2FixtureGeneratorTest" --dependency-verification=off`
