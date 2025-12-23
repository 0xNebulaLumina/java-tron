# Old (Core) System Contract Fixture Generators — Detailed TODO

Goal: add Java conformance fixture generators (embedded oracle) for:
- `ACCOUNT_CREATE_CONTRACT` (0)
- `TRANSFER_CONTRACT` (1)
- `TRANSFER_ASSET_CONTRACT` (2)
- `VOTE_WITNESS_CONTRACT` (4)
- `WITNESS_CREATE_CONTRACT` (5)
- `ASSET_ISSUE_CONTRACT` (6)
- `WITNESS_UPDATE_CONTRACT` (8)
- `ACCOUNT_UPDATE_CONTRACT` (10)
- `FREEZE_BALANCE_CONTRACT` (11)
- `UNFREEZE_BALANCE_CONTRACT` (12)
- `WITHDRAW_BALANCE_CONTRACT` (13)
- `FREEZE_BALANCE_V2_CONTRACT` (54)
- `UNFREEZE_BALANCE_V2_CONTRACT` (55)

This is *only* for Java fixture generation under `framework/src/test/java/org/tron/core/conformance/` that writes fixtures to `conformance/fixtures/`.

## Key References (read before coding)

Conformance framework:
- Java fixture generator: `framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java`
- Java metadata model: `framework/src/test/java/org/tron/core/conformance/FixtureMetadata.java`
- KV format: `framework/src/test/java/org/tron/core/conformance/KvFileFormat.java`
- Existing fixture generators (patterns to follow):
  - `framework/src/test/java/org/tron/core/conformance/AccountFixtureGeneratorTest.java`
  - `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java`
  - `framework/src/test/java/org/tron/core/conformance/Trc10ExtensionFixtureGeneratorTest.java`

Actuator semantics (embedded oracle):
- AccountCreate: `actuator/src/main/java/org/tron/core/actuator/CreateAccountActuator.java`
- Transfer: `actuator/src/main/java/org/tron/core/actuator/TransferActuator.java`
- TransferAsset: `actuator/src/main/java/org/tron/core/actuator/TransferAssetActuator.java`
- VoteWitness: `actuator/src/main/java/org/tron/core/actuator/VoteWitnessActuator.java`
- WitnessCreate/Update: `actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java`, `actuator/src/main/java/org/tron/core/actuator/WitnessUpdateActuator.java`
- AssetIssue: `actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java`
- AccountUpdate: `actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`
- Freeze/Unfreeze V1: `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceActuator.java`, `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceActuator.java`
- WithdrawBalance: `actuator/src/main/java/org/tron/core/actuator/WithdrawBalanceActuator.java`
- Freeze/Unfreeze V2: `actuator/src/main/java/org/tron/core/actuator/FreezeBalanceV2Actuator.java`, `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceV2Actuator.java`

Rust conformance runner expectations (why request encoding matters):
- Fixture runner: `rust-backend/crates/core/src/conformance/runner.rs`
- Non-VM dispatch + unwrap-Any behavior: `rust-backend/crates/core/src/service/mod.rs`
- Transfer/TRC10 handlers requiring `to`/`value`/`asset_id`: `rust-backend/crates/core/src/service/mod.rs`
- Freeze V1/V2 handlers parse protobuf bytes in `tx.data`: `rust-backend/crates/core/src/service/contracts/freeze.rs`

Remote request mapping (authoritative for how Rust handlers currently interpret fields):
- Java mapping of `TronTransaction.{to,value,data,asset_id}`: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

## 0) Decisions to Lock Early (avoid rework)

- [ ] **Fixture request encoding policy**: make fixture `request.pb` match `RemoteExecutionSPI` mapping (recommended), not “naive Any bytes”.
  - Rationale: many Rust handlers (Transfer/TransferAsset/AccountUpdate/WitnessCreate/WitnessUpdate) expect `tx.to`, `tx.value`, `tx.asset_id`, or raw `tx.data` bytes, and will fail if we send `google.protobuf.Any` bytes.
- [ ] **Strictness of `expectedErrorMessage`**:
  - Current generator overwrites `FixtureMetadata.expectedErrorMessage` with the *full* exception message from Java (`FixtureGenerator.generate()` does this).
  - Rust conformance checks `actual_error.contains(expectedErrorMessage)` (see `rust-backend/crates/core/src/conformance/runner.rs`), so:
    - Keeping the full Java message makes fixtures a stricter oracle (Rust must include that exact substring).
    - Storing a short substring is less brittle, but requires changing generator logic (and likely regenerating existing fixtures for consistency).
- [ ] **Default dynamic property “mode matrix”** for core fixtures:
  - `saveChangeDelegation(0)` for all vote/unfreeze/withdraw fixtures to avoid delegation-cycle side effects.
  - For TRC‑10 fixtures: default to `saveAllowSameTokenName(1)` (V2 id semantics).
  - For Freeze V1 fixtures: `saveUnfreezeDelayDays(0)` (V1 is rejected when V2 unfreeze delay is enabled).
  - For Freeze/Unfreeze V2 fixtures: `saveUnfreezeDelayDays(14)` or other >0 value.
  - Decide `saveAllowNewResourceModel(0)` unless TRON_POWER cases are in scope.
- [ ] **Determinism approach**:
  - Option A (minimal): accept that request metadata (transaction_id) is time-dependent; rely on DB state parity only.
  - Option B (recommended): make transaction timestamps deterministic in fixture generators (fixed timestamp/expiration), so repeated generation yields stable `request.pb`.

## 0.5 Suggested Rollout Strategy (keeps failures debuggable)

- [ ] Phase A (plumbing + smoke):
  - [ ] Fix `buildRequest()` mapping + add missing store iterators first.
  - [ ] Generate **one** `happy_path` fixture per contract type to validate request encoding + DB capture end-to-end.
  - [ ] Run Rust conformance and fix obvious request/DB-name issues (before adding many failing validation cases).
- [ ] Phase B (validation matrix):
  - [ ] Add 2–4 `validate_fail_*` cases per contract, prioritizing “common user mistakes” and core invariants.
  - [ ] Expect many Rust mismatches initially (messages + semantics); iterate contract-by-contract.
- [ ] Phase C (edge cases):
  - [ ] Add “creates recipient”, “sweeps expired unfrozen”, multi-vote, etc once Phase A/B are stable.

## 1) CRITICAL Plumbing: Fix `FixtureGenerator.buildRequest()` Mapping

Today `FixtureGenerator.buildRequest()` always sets `to = empty`, `value = 0`, `asset_id = empty`, and `data = contract.parameter (Any bytes)`. This is insufficient for multiple contracts.

Target: produce a `backend.proto` `TronTransaction` that Rust conformance executes the same way the real remote path does.

### 1.1 TODO: Implement contract-aware mapping (minimum set)

File: `framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java` (method `buildRequest`)

- [ ] Implement `switch (contract.getType())` and set:
  - [ ] `TRANSFER_CONTRACT`:
    - `to = transfer.to_address`
    - `value = transfer.amount`
    - `data = empty` (to match `RemoteExecutionSPI`)
  - [ ] `TRANSFER_ASSET_CONTRACT`:
    - `to = transfer_asset.to_address`
    - `value = transfer_asset.amount`
    - `asset_id = transfer_asset.asset_name` bytes
    - `data = empty`
  - [ ] `ACCOUNT_UPDATE_CONTRACT`:
    - `from = account_update.owner_address` (defensive; should already match `trxCap.getOwnerAddress()`)
    - `data = account_update.account_name` bytes
    - `to = empty`, `value = 0`
  - [ ] `WITNESS_CREATE_CONTRACT`:
    - `data = witness_create.url` bytes
    - `to = empty`, `value = 0`
  - [ ] `WITNESS_UPDATE_CONTRACT`:
    - `data = witness_update.update_url` bytes
    - `to = empty`, `value = 0`
  - [ ] `WITHDRAW_BALANCE_CONTRACT`:
    - `data = empty`, `to = empty`, `value = 0`
- [ ] Ensure `TronTransaction.asset_id` is actually populated:
  - Today `FixtureGenerator.buildRequest()` never calls `setAssetId(...)`, so TRC-10 fixtures can’t work until this is wired.
  - Add a local `byte[] assetId = new byte[0];` and always `.setAssetId(ByteString.copyFrom(assetId))` (set non-empty only for `TRANSFER_ASSET_CONTRACT`).
- [ ] Address encoding guardrails (to avoid “invalid address length” in Rust conformance runner):
  - Rust accepts `20` (EVM) or `21` bytes (`0x41` TRON-prefix) for `from/to/coinbase`.
  - Use the bytes already in proto contracts (`*.getOwnerAddress()/getToAddress()`), which are `0x41`-prefixed.
  - Use *empty* `to` (`byte[0]`) for system contracts so Rust sees `to = None` (do **not** use `new byte[20]` “zero address” for NON_VM system contracts).
- [ ] Ensure remaining targeted contracts still work:
  - [ ] `ACCOUNT_CREATE_CONTRACT`: set `data = account_create.toByteArray()` (raw proto bytes) OR keep Any and let Rust unwrap; pick one and be consistent.
  - [ ] `VOTE_WITNESS_CONTRACT`: set `data = vote_witness.toByteArray()` (raw proto).
  - [ ] `ASSET_ISSUE_CONTRACT`: set `data = asset_issue.toByteArray()` (raw proto).
  - [ ] `FREEZE_BALANCE_CONTRACT`/`UNFREEZE_BALANCE_CONTRACT`: set `data = contract.toByteArray()` (raw proto).
  - [ ] `FREEZE_BALANCE_V2_CONTRACT`/`UNFREEZE_BALANCE_V2_CONTRACT`: set `data = contract.toByteArray()` (raw proto).
    - Note: even though `RemoteExecutionSPI` does not currently map these, Rust handlers exist and expect protobuf bytes in `tx.data`.

### 1.2 TODO: Add guardrails and diagnostics

- [ ] Add explicit logging for request field mapping: `{contract_type, from, to_len, value, data_len, asset_id_len}`.
- [ ] Add “safe defaults” for unsupported contract types:
  - Keep current behavior (Any bytes) but log warning that request encoding is likely incomplete.

### 1.3 (Optional) Add a tiny test to prevent regressions

This repo doesn’t currently have unit tests for `FixtureGenerator.buildRequest`, but a small JUnit test would catch the most dangerous regressions:
- [ ] Add `framework/src/test/java/org/tron/core/conformance/FixtureGeneratorRequestMappingTest.java`:
  - [ ] Construct fake `TransactionCapsule` for TransferContract and verify `request.transaction.to` and `request.transaction.value` are populated.
  - [ ] Construct for AccountUpdateContract and verify `request.transaction.data == account_name bytes` (not protobuf/Any bytes).
  - [ ] Construct for TransferAssetContract and verify `asset_id` is set.

## 2) Plumbing: DB Capture Coverage (stores to snapshot)

### 2.1 TODO: Add missing store iterator(s)

File: `framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java` (method `getStoreIterator`)

- [ ] Add support for `account-index`:
  - `case "account-index": return convertIterator(chainBaseManager.getAccountIndexStore().iterator());`
  - Rationale: `AccountUpdateContract` writes to `AccountIndexStore` (`actuator/src/main/java/org/tron/core/actuator/UpdateAccountActuator.java`).

### 2.2 TODO: Re-audit DB mapping list

- [ ] Confirm all DB names referenced in new fixture metadata exist in `getStoreIterator`:
  - Already present: `account`, `witness`, `votes`, `asset-issue-v2`, `dynamic-properties`, `DelegatedResource*`, etc.
- [ ] Decide if any new fixtures should include:
  - `delegation` (only if `saveChangeDelegation(1)` or reward paths are in scope)
  - `asset-issue` (if testing `allowSameTokenName=0` asset issuance behavior)

## 3) Shared Fixture Test Support (reduce copy/paste + increase determinism)

Recommended (but optional) helper: `framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java`

- [ ] Add helper methods:
  - [ ] `createTransaction(ContractType type, Message contract, long timestampMs, long expirationMs)` (deterministic timestamps)
  - [ ] `createBlockContext(String witnessHexAddress, long blockNumber, long blockTimestampMs)`
  - [ ] `putAccount(String hexAddr, long balanceSun)` returning `AccountCapsule`
  - [ ] `putWitness(String hexAddr, String url, long voteCount)`
  - [ ] `setCommonDynamicPropsBaseline(Manager dbManager, long headBlockNum, long headBlockTime)`
    - Must set values that are read by getters (avoid “not found KEY” exceptions).
- [ ] Adopt helper in new generator tests (and optionally refactor existing ones later).

Determinism TODO:
- [ ] Stop using `System.currentTimeMillis()` in fixture generator transaction creation (only in conformance tests).
  - Prefer: `timestamp = fixed`, `expiration = fixed + 3600000`, and choose blockTimestamp accordingly.

## 4) New Generator Test Classes (where the fixtures live)

Naming convention: classes must match `*FixtureGeneratorTest` so the script `scripts/ci/run_fixture_conformance.sh` can pick them up.

Planned classes (suggested grouping; adjust as needed):
- [ ] `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`:
  - `ACCOUNT_CREATE_CONTRACT`, `ACCOUNT_UPDATE_CONTRACT`
- [ ] `framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java`:
  - `TRANSFER_CONTRACT`, `TRANSFER_ASSET_CONTRACT`
- [ ] `framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java`:
  - `VOTE_WITNESS_CONTRACT`, `WITNESS_CREATE_CONTRACT`, `WITNESS_UPDATE_CONTRACT`, `WITHDRAW_BALANCE_CONTRACT`
- [ ] `framework/src/test/java/org/tron/core/conformance/AssetIssueFixtureGeneratorTest.java`:
  - `ASSET_ISSUE_CONTRACT`
- [ ] `framework/src/test/java/org/tron/core/conformance/FreezeV1FixtureGeneratorTest.java`:
  - `FREEZE_BALANCE_CONTRACT`, `UNFREEZE_BALANCE_CONTRACT`
- [ ] `framework/src/test/java/org/tron/core/conformance/FreezeV2FixtureGeneratorTest.java` (or extend existing resource generator):
  - `FREEZE_BALANCE_V2_CONTRACT`, `UNFREEZE_BALANCE_V2_CONTRACT`

## 5) Fixture Case Specifications (per contract)

Notes:
- Case directory = `contract_type.toLowerCase() + "/" + caseName`
- Case categories used by existing fixtures: `happy`, `validate_fail`, `edge`
- Prefer snake_case case names to match existing conventions.

### 5.1 `ACCOUNT_CREATE_CONTRACT` (0)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- Owner exists, balance >= `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`.
- Target account absent.
- Set `saveCreateNewAccountFeeInSystemContract(fee)` and `saveAllowMultiSign(0)` to reduce account-permissions variability.

TODO fixtures:
- [ ] `happy_path_create_account`
  - owner has sufficient balance, target absent, valid target address.
- [ ] `validate_fail_owner_missing`
  - delete owner account before tx.
- [ ] `validate_fail_account_exists`
  - pre-create target account.
- [ ] `validate_fail_insufficient_fee`
  - owner balance < fee.

### 5.2 `TRANSFER_CONTRACT` (1)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- Owner exists with balance.
- Set `saveCreateNewAccountFeeInSystemContract(fee)` so “create recipient” path is stable.
- Keep `saveAllowBlackHoleOptimization(0)` (credit blackhole instead of burn) for determinism unless explicitly testing burn.

TODO fixtures:
- [ ] `happy_path_existing_recipient`
  - both accounts exist, amount > 0, owner != to.
- [ ] `happy_path_creates_recipient`
  - recipient absent; expect account auto-created + extra fee.
- [ ] `validate_fail_to_self`
  - to == owner.
- [ ] `validate_fail_amount_zero`
  - amount = 0.
- [ ] `validate_fail_insufficient_balance`
  - owner balance < amount (+ fee if recipient absent).

### 5.3 `TRANSFER_ASSET_CONTRACT` (2)

DBs: `account`, `asset-issue-v2`, `dynamic-properties`

Pre-state baseline (recommended):
- `saveAllowSameTokenName(1)` so `asset_name` is treated as token id bytes.
- Seed `asset-issue-v2` with an `AssetIssueCapsule` for token id `"1000001"` (or similar).
- Give owner account `assetV2["1000001"] = N` and enough TRX if recipient is missing (create-account-fee path).

TODO fixtures:
- [ ] `happy_path_transfer_asset_existing_recipient`
  - recipient exists; transfer amount <= owner asset balance.
- [ ] `happy_path_transfer_asset_creates_recipient`
  - recipient absent; ensure owner has enough TRX for create account fee.
- [ ] `validate_fail_asset_not_found`
  - token id does not exist in `asset-issue-v2`.
- [ ] `validate_fail_insufficient_asset_balance`
  - owner asset balance < amount.
- [ ] `validate_fail_to_self`
  - to == owner.

Optional coverage (explicitly decide):
- [ ] Add a separate case family for `saveAllowSameTokenName(0)` (legacy name-based semantics), which touches `asset-issue` and `account.asset` map.

### 5.4 `VOTE_WITNESS_CONTRACT` (4)

DBs: `account`, `votes`, `witness`, `dynamic-properties`

Pre-state baseline:
- `saveChangeDelegation(0)` to keep `mortgageService.withdrawReward()` a no-op.
- Create:
  - voter account with TRON power via frozen balances (directly seed AccountCapsule frozen fields; do *not* rely on running freeze).
  - candidate account + candidate witness entry.
- Keep votes list empty initially unless testing vote replacement.

TODO fixtures:
- [ ] `happy_path_single_vote`
  - votesCount=1, voteCount=1 (TRX), tronPower >= 1_000_000 SUN.
- [ ] `validate_fail_vote_count_zero`
  - vote_count=0 in the one vote.
- [ ] `validate_fail_votes_empty`
  - no votes entries.
- [ ] `validate_fail_candidate_not_witness`
  - candidate account exists but no witness entry.
- [ ] `validate_fail_votes_exceed_tron_power`
  - set tronPower small and voteCount large.

Optional coverage:
- [ ] `validate_fail_too_many_votes` (31 entries) to cover `MAX_VOTE_NUMBER`.

### 5.5 `WITNESS_CREATE_CONTRACT` (5)

DBs: `account`, `witness`, `dynamic-properties`

Pre-state baseline:
- Owner account exists with balance >= `AccountUpgradeCost`.
- Owner is not already witness.
- Set dynamic props accessed:
  - `saveAccountUpgradeCost(cost)`
  - `saveAllowMultiSign(0)` (unless explicitly testing default witness permission)
  - `saveAllowBlackHoleOptimization(0 or 1)` (pick one)
  - Ensure `TOTAL_CREATE_WITNESS_COST` key is initialized (if defaults are not guaranteed, explicitly `saveTotalCreateWitnessFee(0)`).

TODO fixtures:
- [ ] `happy_path_create_witness`
- [ ] `validate_fail_invalid_url`
  - empty URL or >256 bytes (match actuator’s `TransactionUtil.validUrl`).
- [ ] `validate_fail_witness_exists`
  - pre-create witness entry.
- [ ] `validate_fail_insufficient_balance`
  - owner balance < upgrade cost.

### 5.6 `WITNESS_UPDATE_CONTRACT` (8)

DBs: `witness`, `account`

Pre-state baseline:
- Account exists.
- Witness exists for same address.

TODO fixtures:
- [ ] `happy_path_update_url`
- [ ] `validate_fail_invalid_url`
- [ ] `validate_fail_witness_missing`
  - account exists, witness missing.
- [ ] `validate_fail_account_missing`

### 5.7 `ASSET_ISSUE_CONTRACT` (6)

DBs: `account`, `asset-issue-v2`, `dynamic-properties` (+ optionally `asset-issue`)

Pre-state baseline (recommended minimal):
- `saveAllowSameTokenName(1)` to keep output in `asset-issue-v2`.
- Set dynamic props required by validation:
  - `saveLatestBlockHeaderTimestamp(headTs)`
  - `saveAssetIssueFee(fee)`
  - `saveTokenIdNum(n)` (starting point)
  - `saveMaxFrozenSupplyNumber(x)` (>=0)
  - `saveOneDayNetLimit(big)`
  - `saveMinFrozenSupplyTime(minDays)` / `saveMaxFrozenSupplyTime(maxDays)` (only relevant if frozen_supply list non-empty; simplest is empty list).
- Ensure start_time > headTs; end_time > start_time.

TODO fixtures:
- [ ] `happy_path_issue_asset_v2`
  - empty frozen_supply list.
- [ ] `validate_fail_total_supply_zero`
- [ ] `validate_fail_start_time_before_head`
- [ ] `validate_fail_owner_already_issued`
  - set owner `asset_issued_name` non-empty (or use an account state that triggers this check).

Optional coverage:
- [ ] `edge_allow_same_token_name_0_writes_v1_and_v2`
  - set `saveAllowSameTokenName(0)`; expect writes to both `asset-issue` and `asset-issue-v2`.

### 5.8 `ACCOUNT_UPDATE_CONTRACT` (10)

DBs: `account`, `account-index`, `dynamic-properties`

Pre-state baseline:
- `saveAllowUpdateAccountName(0)` (default: only set once).
- Ensure `account-index` capture works (see section 2).
- Account exists with empty name for happy path.

TODO fixtures:
- [ ] `happy_path_set_name_first_time`
- [ ] `validate_fail_invalid_name`
  - too short / too long / invalid chars (align `TransactionUtil.validAccountName`).
- [ ] `validate_fail_account_missing`
- [ ] `validate_fail_duplicate_name_updates_disabled`
  - create another account that already uses the name and ensure `AccountIndexStore.has(name)` is true.

Optional coverage:
- [ ] `edge_allow_update_account_name_enabled_allows_second_set`
  - set `saveAllowUpdateAccountName(1)` and attempt second update.

### 5.9 `FREEZE_BALANCE_CONTRACT` (11) (V1)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- Ensure V1 is enabled: `saveUnfreezeDelayDays(0)` (because V2-enabled chain rejects V1 freeze).
- Set totals used during execution:
  - `saveTotalNetWeight(0)`, `saveTotalEnergyWeight(0)`, and optionally `saveTotalTronPowerWeight(0)`.
- Account exists with sufficient balance.
- Avoid delegation receiver address in V1 baseline fixtures (keeps `DelegatedResource*` stores out of scope).

TODO fixtures:
- [ ] `happy_path_freeze_bandwidth_v1`
- [ ] `validate_fail_v1_closed_when_v2_open`
  - set `saveUnfreezeDelayDays(14)` and attempt V1 freeze.
- [ ] `validate_fail_frozen_balance_lt_1_trx`
  - amount < `TRX_PRECISION`.
- [ ] `validate_fail_frozen_balance_gt_balance`

### 5.10 `UNFREEZE_BALANCE_CONTRACT` (12) (V1)

DBs: `account`, `votes`, `dynamic-properties`

Pre-state baseline:
- `saveChangeDelegation(0)` to no-op reward withdraw.
- Create owner account with frozen entry:
  - For happy path: expire_time <= `latestBlockHeaderTimestamp`.
  - For not-expired failure: expire_time > `latestBlockHeaderTimestamp`.
- Initialize totals: `saveTotalNetWeight(0)` etc as needed.

TODO fixtures:
- [ ] `happy_path_unfreeze_bandwidth_v1`
  - ensures at least one frozen entry has expired.
- [ ] `validate_fail_not_expired`
- [ ] `validate_fail_no_frozen_balance`

Optional:
- [ ] ENERGY resource unfreeze case.
- [ ] TRON_POWER resource unfreeze case (requires `saveAllowNewResourceModel(1)`).

### 5.11 `WITHDRAW_BALANCE_CONTRACT` (13)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- `saveChangeDelegation(0)` so `mortgageService.withdrawReward/queryReward` are no-ops.
- Set timestamp high enough to pass cooldown:
  - set `saveWitnessAllowanceFrozenTime(1)` and `saveLatestBlockHeaderTimestamp(>= 86400000)`.
- Account exists with:
  - `allowance > 0`
  - `latestWithdrawTime = 0` (or sufficiently old)
- Ensure owner is not a guard representative (use non-genesis witness address).

TODO fixtures:
- [ ] `happy_path_withdraw_allowance`
- [ ] `validate_fail_too_soon`
  - `latestWithdrawTime` near now so cooldown fails.
- [ ] `validate_fail_no_reward`
  - allowance = 0 (and changeDelegation=0 so queryReward=0).

### 5.12 `FREEZE_BALANCE_V2_CONTRACT` (54)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- Enable V2: `saveUnfreezeDelayDays(14)` (any >0).
- Initialize totals: `saveTotalNetWeight(0)`, `saveTotalEnergyWeight(0)`.
- Account exists with sufficient balance.

TODO fixtures:
- [ ] `happy_path_freeze_v2_bandwidth`
- [ ] `validate_fail_feature_not_enabled`
  - set `saveUnfreezeDelayDays(0)` and attempt V2 freeze.
- [ ] `validate_fail_frozen_balance_gt_balance`

### 5.13 `UNFREEZE_BALANCE_V2_CONTRACT` (55)

DBs: `account`, `dynamic-properties`

Pre-state baseline:
- Enable V2: `saveUnfreezeDelayDays(14)` (>0).
- Seed account frozenV2 list with sufficient balance for BANDWIDTH.
- Keep account votes list empty (avoid `votes` store).
- Set `latestBlockHeaderTimestamp` for deterministic `unfreezeExpireTime`.

TODO fixtures:
- [ ] `happy_path_unfreeze_v2_bandwidth`
  - unfreeze_balance <= frozen amount, count of unfreezing entries < 32.
- [ ] `validate_fail_no_frozen_balance`
- [ ] `validate_fail_unfreeze_balance_too_high`
- [ ] `edge_sweep_expired_unfrozen_v2`
  - seed an expired `unfrozenV2` entry so execute sweeps it into balance; capture `withdrawExpireAmount` effect via account state.

## 6) Sanity Checks / Runbook

Generation:
- [ ] Run: `./gradlew :framework:test --tests "*FixtureGeneratorTest*" -Dconformance.output=../conformance/fixtures --dependency-verification=off`
  - If iterating on one contract, add `--tests "TransferFixtureGeneratorTest"` etc.

Post-generation:
- [ ] Confirm new directories exist in `conformance/fixtures/`:
  - `account_create_contract/`, `transfer_contract/`, `transfer_asset_contract/`, etc.
- [ ] For a sample case, confirm fixture has:
  - `request.pb`
  - `pre_db/*.kv`
  - `expected/post_db/*.kv`
  - `metadata.json`
- [ ] Verify `request.pb` fields are correct for Transfer/TransferAsset/AccountUpdate/WitnessCreate/WitnessUpdate:
  - Transfer: `to` non-empty and `value` non-zero.
  - TransferAsset: `to` non-empty, `value` non-zero, `asset_id` non-empty.
  - AccountUpdate: `data` length <= 32 (account name bytes).
  - WitnessCreate/Update: `data` is URL bytes (not protobuf bytes).

Rust conformance run:
- [ ] `./scripts/ci/run_fixture_conformance.sh` (or `--generate-only` / `--rust-only`)
  - If Rust doesn’t yet support some families, add them to the exclude list in the script temporarily.

## 7) Optional: Update Documentation

- [ ] Update `conformance/README.md` coverage table to include these contract types and generator status.
