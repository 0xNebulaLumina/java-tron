add fixture generators for
+ ACCOUNT_CREATE_CONTRACT
+ TRANSFER_CONTRACT
+ TRANSFER_ASSET_CONTRACT
+ VOTE_WITNESS_CONTRACT
+ WITNESS_CREATE_CONTRACT
+ ASSET_ISSUE_CONTRACT
+ WITNESS_UPDATE_CONTRACT
+ ACCOUNT_UPDATE_CONTRACT
+ FREEZE_BALANCE_CONTRACT
+ UNFREEZE_BALANCE_CONTRACT
+ WITHDRAW_BALANCE_CONTRACT
+ FREEZE_BALANCE_V2_CONTRACT
+ UNFREEZE_BALANCE_V2_CONTRACT

---

• What Exists Today

- Java fixture generation lives in JUnit tests under framework/src/test/java/org/tron/core/conformance/:1 and writes fixtures to conformance/fixtures/:1 via FixtureGenerator (framework/src/test/java/org/tron/
core/conformance/FixtureGenerator.java:1).
- Each generator test:
    - seeds DB state (accounts, witnesses, dynamic props, etc),
    - builds a TransactionCapsule with Any.pack(contractProto),
    - calls generator.generate(trx, block, metadata) to capture pre_db/*.kv, request.pb, expected/post_db/*.kv, expected/result.pb, metadata.json.
- Missing for your list: no generator coverage yet for contract types 0,1,2,4,5,6,8,10,11,12,13,54,55 (see enum in protocol/src/main/protos/core/Tron.proto:1).

High-Level Plan (Files/Structure)

- Add iterator support needed for new fixtures:
    - Update framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java:1 to support capturing account-index (used by AccountUpdateContract).
- Add new generator test classes (keep them small and scoped, like existing *FixtureGeneratorTest files):
    - framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java:1 (TRANSFER + TRANSFER_ASSET)
    - framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java:1 (ACCOUNT_CREATE + ACCOUNT_UPDATE)
    - framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java:1 (VOTE_WITNESS + WITNESS_CREATE + WITNESS_UPDATE + WITHDRAW_BALANCE)
    - framework/src/test/java/org/tron/core/conformance/AssetIssueFixtureGeneratorTest.java:1 (ASSET_ISSUE)
    - framework/src/test/java/org/tron/core/conformance/FreezeV1FixtureGeneratorTest.java:1 (FREEZE_BALANCE + UNFREEZE_BALANCE)
    - Extend framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java:1 (already V2-aware) to add FREEZE_BALANCE_V2 + UNFREEZE_BALANCE_V2, or create
FreezeV2FixtureGeneratorTest.java:1.
- Optional but recommended: add a tiny shared helper (new class) to reduce duplication across generators:
    - framework/src/test/java/org/tron/core/conformance/ConformanceFixtureTestSupport.java:1 with:
        - createTransaction(type, msg), createBlockContext(witnessAddr)
        - initCommonDynamicPropsV1() vs initCommonDynamicPropsV2()
        - putAccount(addr, balance, extraProtoMutations) / putWitness(addr, url, voteCount) / putAssetIssueV2(tokenId, ...)

Common Setup Rules (To Avoid Flaky/Invalid Fixtures)

- Always set the dynamic properties that these actuators read (many getters throw if unset); do it explicitly in each generator’s initializeTestData():
    - latestBlockHeaderTimestamp, latestBlockHeaderNumber
    - allowBlackHoleOptimization (burn-vs-blackhole paths)
    - allowMultiSign (affects default permissions on new accounts/witness)
    - createNewAccountFeeInSystemContract (transfer-to-new-account + account create fees)
    - changeDelegation (needed because Vote/Unfreeze/Withdraw call mortgageService.withdrawReward; set 0 unless you explicitly want reward/delegation-state fixtures)
- Split Freeze V1 vs V2 environments:
    - V1 freeze fixtures must set unfreezeDelayDays = 0 (V2 enabled blocks V1 freeze).
    - V2 freeze/unfreeze fixtures must set unfreezeDelayDays > 0.
- Keep stores small/deterministic:
    - Create only the accounts/witnesses/assets needed for the case.
    - Prefer unique addresses per test case to avoid cross-test contamination inside the same class DB.

Per-Contract Fixture Matrix (What To Generate)
Below is a “minimum good” set: 1 happy + 2–3 validate_fail per contract, with the DBs you must capture to catch state divergences.

- ACCOUNT_CREATE_CONTRACT (type=0)
    - Happy: happy_path_create_account — owner exists + balance ≥ create-account-fee, new account absent.
        - DBs: account, dynamic-properties
    - Fail: validate_fail_owner_missing — owner absent.
    - Fail: validate_fail_account_exists — target already exists.
    - Fail: validate_fail_insufficient_fee — owner balance < fee.
- TRANSFER_CONTRACT (type=1)
    - Happy: happy_path_existing_recipient — normal transfer to existing account.
        - DBs: account, dynamic-properties
    - Happy: happy_path_creates_recipient — recipient absent; validates/executes extra create-account-fee path.
        - DBs: account, dynamic-properties
    - Fail: validate_fail_insufficient_balance
    - Fail: validate_fail_to_self
    - Fail: validate_fail_amount_zero
- TRANSFER_ASSET_CONTRACT (type=2)
    - Pre-state requirement: TRC-10 asset exists in the final asset store for your allowSameTokenName mode (recommend allowSameTokenName=1, seed asset-issue-v2 with token id like "1000001").
    - Happy: happy_path_transfer_asset — to-account exists, fee=0 path.
        - DBs: account (optionally include asset-issue-v2 for safety)
    - Happy: happy_path_creates_recipient — to-account absent; consumes create-account-fee.
        - DBs: account, dynamic-properties (and optionally asset-issue-v2)
    - Fail: validate_fail_asset_not_found — token id missing from store.
    - Fail: validate_fail_insufficient_asset_balance
    - Fail: validate_fail_to_self
- VOTE_WITNESS_CONTRACT (type=4)
    - Pre-state requirements:
        - voter account exists and has sufficient TRON power (seed frozen balances directly; don’t rely on running freeze inside the fixture unless you want multi-tx setup),
        - vote target exists in both account and witness.
        - Set changeDelegation=0 unless you want delegation-cycle reward mutations in the fixture.
    - Happy: happy_path_single_vote — voteCount=1 TRX (will write votes to account and votes store).
        - DBs: account, votes (optionally witness, dynamic-properties)
    - Fail: validate_fail_vote_count_zero
    - Fail: validate_fail_candidate_not_witness
    - Fail: validate_fail_votes_exceed_tron_power
- WITNESS_CREATE_CONTRACT (type=5)
    - Pre-state requirements: owner account exists, witness absent, accountUpgradeCost set, totalCreateWitnessCost initialized, burn/blackhole config set.
    - Happy: happy_path_create_witness
        - DBs: account, witness, dynamic-properties
    - Fail: validate_fail_invalid_url
    - Fail: validate_fail_witness_exists
    - Fail: validate_fail_insufficient_balance
- ASSET_ISSUE_CONTRACT (type=6)
    - Pre-state requirements:
        - owner account exists and hasn’t issued an asset yet,
        - dynamic props: allowSameTokenName, assetIssueFee, tokenIdNum, maxFrozenSupplyNumber, oneDayNetLimit, minFrozenSupplyTime, maxFrozenSupplyTime, latestBlockHeaderTimestamp,
        - pick start_time > latestBlockHeaderTimestamp, end_time > start_time.
    - Happy: happy_path_issue_asset_v2 (use allowSameTokenName=1, name != “trx”)
        - DBs: account, asset-issue-v2, dynamic-properties
    - Fail: validate_fail_start_time_before_head
    - Fail: validate_fail_total_supply_zero
    - Fail: validate_fail_already_issued_asset
- WITNESS_UPDATE_CONTRACT (type=8)
    - Happy: happy_path_update_url
        - DBs: witness
    - Fail: validate_fail_not_witness
    - Fail: validate_fail_account_missing
    - Fail: validate_fail_invalid_url
- ACCOUNT_UPDATE_CONTRACT (type=10)
    - Requires capturing account-index to verify name index mutations.
    - Happy: happy_path_set_name_first_time (allowUpdateAccountName=0, account name initially empty)
        - DBs: account, account-index, dynamic-properties
    - Fail: validate_fail_invalid_name
    - Fail: validate_fail_account_missing
    - Fail: validate_fail_duplicate_name_updates_disabled
- FREEZE_BALANCE_CONTRACT (type=11) (V1)
    - Pre-state requirements:
        - unfreezeDelayDays=0 (V2 must be “off”),
        - initialize totals (totalNetWeight, totalEnergyWeight, optionally totalTronPowerWeight),
        - set allowNewReward (recommend 0 for predictable weight math).
    - Happy: happy_path_freeze_bandwidth_v1
        - DBs: account, dynamic-properties
    - Fail: validate_fail_v1_closed_when_v2_open (set unfreezeDelayDays > 0)
    - Fail: validate_fail_frozen_balance_lt_1_trx
    - Fail: validate_fail_frozen_balance_gt_balance
- UNFREEZE_BALANCE_CONTRACT (type=12) (V1)
    - Pre-state requirements:
        - seed an expired frozen entry (expireTime <= latestBlockHeaderTimestamp) for the chosen resource,
        - set changeDelegation=0 (avoids reward cycle DB churn),
        - set totals + allowNewReward.
    - Happy: happy_path_unfreeze_bandwidth_v1 (note: will also write votes store even if votes empty)
        - DBs: account, votes, dynamic-properties
    - Fail: validate_fail_not_expired
    - Fail: validate_fail_no_frozen_balance
- WITHDRAW_BALANCE_CONTRACT (type=13)
    - Pre-state requirements:
        - changeDelegation=0 (or fully seed delegation cycles),
        - witnessAllowanceFrozenTime set,
        - ensure now - latestWithdrawTime >= witnessAllowanceFrozenTime * 86400000,
        - set allowance > 0 to avoid reward-path complexity.
    - Happy: happy_path_withdraw_allowance
        - DBs: account, dynamic-properties
    - Fail: validate_fail_too_soon
    - Fail: validate_fail_no_reward
- FREEZE_BALANCE_V2_CONTRACT (type=54)
    - Pre-state requirements: unfreezeDelayDays > 0, initialize total weights.
    - Happy: happy_path_freeze_v2_bandwidth
        - DBs: account, dynamic-properties
    - Fail: validate_fail_feature_not_enabled (unfreezeDelayDays=0)
    - Fail: validate_fail_frozen_balance_gt_balance
- UNFREEZE_BALANCE_V2_CONTRACT (type=55)
    - Pre-state requirements:
        - unfreezeDelayDays > 0,
        - seed frozenV2 entry for resource with amount,
        - keep votes list empty for simplest fixture (avoids votes store writes).
    - Happy: happy_path_unfreeze_v2_bandwidth
        - DBs: account, dynamic-properties
    - Edge: edge_with_expired_unfrozen_v2_swept (seed an expired unfrozenV2 entry to test withdrawExpireAmount)
    - Fail: validate_fail_no_frozen_balance
    - Fail: validate_fail_unfreeze_balance_too_high

Generation/CI Workflow

- Run only generation (Java): ./gradlew :framework:test --tests "*FixtureGeneratorTest*" -Dconformance.output=../conformance/fixtures --dependency-verification=off (pattern used by scripts/ci/
run_fixture_conformance.sh:1).
- Validate Rust conformance on the expanded fixture set: ./scripts/ci/run_fixture_conformance.sh (may need to adjust its “unsupported fixture families” filter if Rust isn’t ready for some).
- After fixtures are in place, consider updating conformance/README.md:1 contract coverage table to include these families and their status.

