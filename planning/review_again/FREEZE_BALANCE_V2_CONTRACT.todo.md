# TODO / Fix plan: FREEZE_BALANCE_V2_CONTRACT (Rust vs Java parity)

## Goal
Bring Rust backend `FREEZE_BALANCE_V2_CONTRACT` behavior into parity with java-tron, especially for:
- **V2 “no expiration” semantics** in freeze reporting (CSV/domain deltas).
- Consistent transaction decoding expectations (`data` vs `from_raw`) and test coverage.

## Acceptance criteria (what “fixed” means)
- [ ] Rust execution succeeds on valid V2 freeze fixtures and matches Java state:
  - [ ] `Account.balance` decreases by `frozen_balance`
  - [ ] `Account.frozenV2` amount increases (aggregated by type)
  - [ ] `DynamicProperties` total weight delta matches Java calculation
  - [ ] `old_tron_power` initialization matches Java (`0 -> getTronPower(); 0 => -1`)
- [ ] Remote-mode CSV/domain output matches embedded Java for V2 freeze:
  - [ ] `FreezeDelta.newExpireTimeMs == 0` for V2 freeze
  - [ ] `FreezeDelta.op == "freeze"` (already enforced by `ExecutionCsvRecordBuilder`)
- [ ] Rust unit tests and conformance fixtures pass.

## Work plan (detailed checklist)

### 1) Confirm Java semantics and where they’re enforced
- [ ] Re-verify Java’s “V2 has no expiration” behavior:
  - [ ] `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java` records expireTime `(0,0)`
  - [ ] `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` snapshot capture uses `oldExpireTimeMs = 0` for v2
  - [ ] `framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java` uses `FreezeLedgerChange.expirationMs` as `newExpireTimeMs`
- [ ] Decide the correct remote reporting value for V2: **always `expiration_ms = 0`**.

### 2) Fix Rust: V2 expiration should be 0 (not synthetic)
- [ ] In `rust-backend/crates/core/src/service/contracts/freeze.rs`:
  - [ ] Remove the synthesized `block_timestamp + 3 days` for V2 freezes.
  - [ ] Ensure any persisted freeze-ledger record expiration for V2 is `0` (or ignored).
  - [ ] Ensure emitted `FreezeLedgerChange.expiration_ms` for v2_model=true is `0`.
- [ ] Ensure `UNFREEZE_BALANCE_V2_CONTRACT` keeps/propagates v2 expiration as `0` (it currently preserves “existing expiration”).

### 3) Make V2 freeze change emission robust (avoid stale custom ledger)
Reason: Rust currently derives emitted `FreezeLedgerChange.amount` from the custom `freeze-records` DB, which can be stale/missing if:
- execution ever falls back to Java, or
- state is migrated without backfilling `freeze-records`.

Choose one (or combine):
- [ ] **Preferred:** compute emitted `amount` from the updated account proto (`new_owner_proto.frozen_v2` sum for the resource type) and emit that (absolute).
  - [ ] Optionally keep writing `freeze-records` as an internal cache, but do not depend on it for emissions.
- [ ] Alternative: treat `freeze-records` as canonical and implement a **backfill/migration** from account state on first read/missing key.

### 4) Clarify/standardize owner address decoding for V2 freeze
- [ ] Decide the canonical source of owner address inside Rust execution:
  - [ ] Option A: use `transaction.metadata.from_raw`/`transaction.from` like other system contracts; ignore `owner_address` in data.
  - [ ] Option B: parse `owner_address` from `transaction.data` and validate against `from_raw` if present.
  - [ ] Option C: accept both (if data has owner_address use it; otherwise fall back to from_raw).
- [ ] Update comments (`parse_freeze_balance_v2_params`) to match the chosen behavior.

### 5) Update Rust tests (they currently don’t reflect production decoding)
- [ ] Fix `rust-backend/crates/core/src/service/tests/contracts.rs` V2 freeze test vector:
  - [ ] Include valid owner_address bytes in `data` **or** set metadata `from_raw` appropriately, depending on decoding decision.
  - [ ] Change the assertion to expect `freeze_change.expiration_ms == 0` for V2.
- [ ] Add/adjust tests for:
  - [ ] “second freeze” (old>0, new>old) emits correct absolute `amount`
  - [ ] expiration always 0 for v2_model=true
  - [ ] robustness when `freeze-records` is absent but account has existing `frozenV2` (if emission no longer depends on `freeze-records`)

### 6) Run Rust + Java verification
- [ ] Rust: `cd rust-backend && cargo test -p tron-backend-core service::tests::contracts::test_freeze_balance_v2_emits_with_v2_flag`
- [ ] Java conformance (optional but recommended for parity):
  - [ ] Generate fixtures: `./gradlew :framework:test --tests "org.tron.core.conformance.FreezeV2FixtureGeneratorTest" --dependency-verification=off`
  - [ ] Run Rust conformance runner on generated fixtures and compare CSV/domain digests with embedded results.

### 7) Rollout safety
- [ ] Keep `freeze_balance_v2_enabled` behind config until parity verified.
- [ ] If the custom `freeze-records` DB is persisted in production, document whether it needs backfill and how to perform it before enabling.

## Notes / expected change list (for the eventual patch)
- `rust-backend/crates/core/src/service/contracts/freeze.rs` (V2 expiration + emission logic)
- `rust-backend/crates/core/src/service/tests/contracts.rs` (update V2 freeze tests)
- Possibly: add a small migration/compat path if `freeze-records` must remain authoritative.

