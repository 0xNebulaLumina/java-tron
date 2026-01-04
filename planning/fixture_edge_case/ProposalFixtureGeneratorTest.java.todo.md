# ProposalFixtureGeneratorTest.java – Missing Fixture Edge Cases

Goal
- Expand `framework/src/test/java/org/tron/core/conformance/ProposalFixtureGeneratorTest.java` fixture generation
  so conformance covers key validation branches, boundary conditions, and a few semantic nuances for:
  - `ProposalCreateContract` (16)
  - `ProposalApproveContract` (17)
  - `ProposalDeleteContract` (18)

Non-Goals
- Do not change consensus/validation rules; only add/adjust fixtures to reflect current Java-tron behavior.
- Avoid fork-gated proposal parameters unless fork activation can be made deterministic in tests.

Acceptance Criteria
- Each new fixture directory contains `pre_db/`, `request.pb`, and `expected/post_db/`.
- For validation failures: `metadata.json.expectedStatus == "VALIDATION_FAILED"` and `expectedErrorMessage`
  matches the thrown `ContractValidateException` message (or a stable substring).
- "Happy" fixtures execute successfully and mutate expected DBs (at least `proposal` and `dynamic-properties`).
- `caseCategory`/`description` remain consistent with the observed result (avoid "validate_fail but SUCCESS").

Checklist / TODO

Phase 0 — Confirm Baselines
- [x] Skim validate logic to align fixtures with real branches:
  - [x] `actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/actuator/ProposalDeleteActuator.java`
  - [x] `actuator/src/main/java/org/tron/core/utils/ProposalUtil.java`
- [ ] Run once to confirm current fixtures are produced as intended:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
  - [ ] Spot-check a couple of generated `metadata.json` files for status + error messages.

Phase 1 — ProposalCreateContract (16) Fixtures

Owner address / account / witness validation
- [x] Add `validate_fail_owner_address_invalid_short`:
  - [x] `owner_address = ByteString.copyFrom(ByteArray.fromHexString("aaaa"))` (2 bytes).
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address (21 bytes, correct prefix) not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` + `"] not exists"`.

Parameter id/value validation (fork-independent; keep deterministic)
- [x] Add `validate_fail_param_code_unsupported`:
  - [x] `parameters = { 9999: 1 }`.
  - [x] Expect validate error contains: `"Does not support code : 9999"`.
- [x] Add `validate_fail_maintenance_interval_too_low`:
  - [x] `parameters = { 0: 1 }` (or any value `< 3*27*1000`).
  - [x] Expect validate error contains: `"MAINTENANCE_TIME_INTERVAL"` or `"valid range"`.
- [x] Add `validate_fail_maintenance_interval_too_high`:
  - [x] `parameters = { 0: 24*3600*1000 + 1 }`.
  - [x] Expect validate error contains: `"valid range"`.
- [x] Add `validate_fail_negative_fee_like_param`:
  - [x] `parameters = { 2: -1 }` (e.g., `CREATE_ACCOUNT_FEE`).
  - [x] Expect validate error contains: `"valid range is [0,"`.
- [x] Add `validate_fail_allow_creation_of_contracts_value_zero`:
  - [x] `parameters = { 9: 0 }`.
  - [x] Expect validate error contains: `"ALLOW_CREATION_OF_CONTRACTS"` and `"only allowed to be 1"`.

Parameter prerequisite / dependency (fork-independent)
- [x] Add `validate_fail_allow_tvm_transfer_trc10_prereq_not_met`:
  - [x] Set `DynamicPropertiesStore.saveAllowSameTokenName(0)` in test setup (or per-test).
  - [x] `parameters = { 18: 1 }`.
  - [x] Expect validate error contains:
    - [x] `"[ALLOW_SAME_TOKEN_NAME] proposal must be approved before [ALLOW_TVM_TRANSFER_TRC10] can be proposed"`.

One-time proposal validation (fork-independent)
- [x] Add `validate_fail_remove_power_gr_already_executed`:
  - [x] Set `DynamicPropertiesStore.saveRemoveThePowerOfTheGr(-1)` before generating fixture.
  - [x] `parameters = { 10: 1 }`.
  - [x] Expect validate error contains: `"only allowed to be executed once"`.
- [x] Add `validate_fail_remove_power_gr_value_not_one`:
  - [x] Set `DynamicPropertiesStore.saveRemoveThePowerOfTheGr(0)` before generating fixture.
  - [x] `parameters = { 10: 0 }` (or `-1`).
  - [x] Expect validate error contains: `"REMOVE_THE_POWER_OF_THE_GR"` and `"only allowed to be 1"`.

Optional boundary-happy fixtures
- [x] Add `happy_path_maintenance_interval_min_bound`:
  - [x] `parameters = { 0: 3*27*1000 }`.
  - [x] Expect `SUCCESS`.
- [x] Add `happy_path_maintenance_interval_max_bound`:
  - [x] `parameters = { 0: 24*3600*1000 }`.
  - [x] Expect `SUCCESS`.

Phase 2 — ProposalApproveContract (17) Fixtures

Owner/witness validation
- [x] Add `validate_fail_owner_address_invalid_short`:
  - [x] `owner_address` 2 bytes (`"aaaa"`).
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` + `"] not exists"`.
- [x] Add `validate_fail_owner_not_witness`:
  - [x] Create an account but do not add it to `WitnessStore`.
  - [x] Attempt approval; expect validate error contains: `"Witness["` + `"] not exists"`.

Proposal store / dynamic property inconsistency (alternate "not exists" branch)
- [x] Add `validate_fail_proposal_missing_but_latest_num_allows_it`:
  - [x] Set `DynamicPropertiesStore.saveLatestProposalNum(100)` and approve `proposalId=100`
        while ensuring `ProposalStore` has no entry for 100.
  - [x] Expect validate error contains: `"Proposal[100] not exists"`.

Expiration boundary
- [x] Add `validate_fail_expired_at_exact_boundary`:
  - [x] Create proposal with `expirationTime == latestBlockHeaderTimestamp` (exact equality).
  - [x] Expect validate error contains: `"expired"`.

Phase 3 — ProposalDeleteContract (18) Fixtures

Owner address / account existence
- [x] Add `validate_fail_owner_address_invalid_short`:
  - [x] `owner_address` 2 bytes (`"aaaa"`).
  - [x] Expect validate error: `"Invalid address"`.
- [x] Add `validate_fail_owner_account_not_exist`:
  - [x] Use a valid-looking address not present in `AccountStore`.
  - [x] Expect validate error contains: `"Account["` + `"] not exists"`.

Semantic nuance: delete does not require witness membership
- [x] Add `happy_path_delete_without_witness_entry`:
  - [x] Ensure owner account exists.
  - [x] Ensure a proposal exists where `proposalAddress == ownerAddress`.
  - [x] Delete owner from `WitnessStore` before generating fixture.
  - [x] Expect `SUCCESS` (this pins Java behavior).

Proposal store / dynamic property inconsistency
- [x] Add `validate_fail_proposal_missing_but_latest_num_allows_it`:
  - [x] Set `DynamicPropertiesStore.saveLatestProposalNum(100)` and delete `proposalId=100`
        while ensuring `ProposalStore` has no entry for 100.
  - [x] Expect validate error contains: `"Proposal[100] not exists"`.

Expiration boundary
- [x] Add `validate_fail_expired_at_exact_boundary`:
  - [x] Create proposal with `expirationTime == latestBlockHeaderTimestamp` (exact equality).
  - [x] Expect validate error contains: `"expired"`.

Optional: cancellation with existing approvals
- [x] Add `happy_path_delete_even_if_already_approved_by_someone`:
  - [x] Create proposal, add an approval from a witness, then delete by creator.
  - [x] Expect `SUCCESS` and state becomes `CANCELED`.

Phase 4 — Hygiene: Make Fixtures Self-Verifying
- [ ] For each "happy" test, assert `result.isSuccess()` after `generator.generate(...)`.
- [ ] For each `validate_fail`, assert `result.getValidationError()` contains a stable substring.
- [ ] Keep fixture DB lists minimal but sufficient:
  - [ ] Always include `proposal` + `dynamic-properties`.
  - [ ] Add `account`/`witness` when the case depends on them.

Phase 5 — Regenerate and Verify Fixtures
- [ ] Run:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.ProposalFixtureGeneratorTest" -Dconformance.output=../conformance/fixtures`
- [ ] Spot-check generated fixtures:
  - [ ] `metadata.json.expectedStatus` matches the case intent.
  - [ ] Error messages are stable (avoid fork-dependent ids unless fork state is pinned).

