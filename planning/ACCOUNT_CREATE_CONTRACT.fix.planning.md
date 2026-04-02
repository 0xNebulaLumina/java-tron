# ACCOUNT_CREATE_CONTRACT missing-dynamic-property parity fix plan

## Assessment

The statements are materially true.

What is already done:

- `ACCOUNT_CREATE_CONTRACT` itself is implemented in Rust and ordinary parity coverage exists.
- Address-prefix parity, account `type` persistence, create-account bandwidth path, and receipt passthrough are already implemented.
- `ALLOW_MULTI_SIGN` already behaves strictly in Rust for this path.

What is still missing:

1. Strict missing-key parity is not fully implemented for `ACCOUNT_CREATE_CONTRACT`.
   - Java throws when critical dynamic properties are absent:
     - `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
     - `ALLOW_MULTI_SIGN`
     - `LATEST_BLOCK_HEADER_TIMESTAMP`
     - `ALLOW_BLACKHOLE_OPTIMIZATION`
     - bandwidth-path keys such as `FREE_NET_LIMIT`, `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`, `CREATE_ACCOUNT_FEE`, `TOTAL_CREATE_ACCOUNT_COST`
   - Rust still falls back for several of those keys in the account-create path:
     - `get_create_new_account_fee_in_system_contract()` defaults to `1_000_000`
     - `get_latest_block_header_timestamp()` defaults to `0`
     - `support_black_hole_optimization()` defaults to `false`
     - tracked-bandwidth getters still default for `FREE_NET_LIMIT`, `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`, `CREATE_ACCOUNT_FEE`, `TOTAL_CREATE_ACCOUNT_COST`
   - The config flag `execution.remote.strict_dynamic_properties` exists, but `execute_account_create_contract()` does not branch on it, and `ConformanceRunner::create_conformance_config()` does not enable it.

2. Missing-key tests are still pending for `ACCOUNT_CREATE_CONTRACT`.
   - Rust has account-create parity tests, but none of them assert missing-key behavior for the account-create contract.
   - Java conformance fixture generation for `account_create_contract` has happy/validate-fail/edge cases, but no missing-dynamic-property cases.

3. Conformance runner updates are still pending for this work.
   - The runner enables `account_create_enabled`, but not strict dynamic-property mode.
   - There is no fixture-level switch for “run this case with strict missing-key parity enabled”.
   - There are no `account_create_contract` missing-key fixtures under `conformance/fixtures/account_create_contract/`.

4. Java-side validation is still pending.
   - There is no `RemoteExecutionSPI` account-create mapping test.
   - `DualStorageModeIntegrationTest` is storage-factory coverage only; it does not validate account-create remote execution parity.
   - There is no focused remote-vs-embedded account-create validation after fixture regeneration.

## Goal

Add opt-in strict missing-key parity for `ACCOUNT_CREATE_CONTRACT`, verify it with Rust unit tests, Java-generated conformance fixtures, Rust conformance execution, and focused Java remote-validation coverage.

## Non-goals

- Do not change the existing non-strict fallback behavior when `strict_dynamic_properties=false`.
- Do not widen this change into a repo-wide strict-dynamic-properties refactor unless needed for shared helpers.
- Do not change current non-missing-key account-create semantics.

## Parity target

Use this rule:

- When `strict_dynamic_properties=false`:
  - preserve today’s fallback behavior for backward compatibility.
- When `strict_dynamic_properties=true`:
  - match Java’s missing-key behavior for every dynamic property actually touched by the executed path.

For `ACCOUNT_CREATE_CONTRACT`, split the keys into two groups.

Always-touched account-create keys:

- `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- `LATEST_BLOCK_HEADER_TIMESTAMP`
- `ALLOW_MULTI_SIGN`
- `ALLOW_BLACKHOLE_OPTIMIZATION`

Tracked-bandwidth/AEXT keys:

- `FREE_NET_LIMIT`
- `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
- `CREATE_ACCOUNT_FEE`
- `TOTAL_CREATE_ACCOUNT_COST`

Keep `ACTIVE_DEFAULT_OPERATIONS` as-is:

- Java already has a default for it.
- Rust already mirrors that default behavior.
- It should not be converted into a strict-missing-key failure.

## Implementation plan

### Phase 1: Add strict getters for the account-create dependency set

Target file:

- `rust-backend/crates/execution/src/storage_adapter/engine.rs`

Work:

- Add strict getter variants for the account-create keys that still fall back today:
  - `get_create_new_account_fee_in_system_contract_strict()`
  - `get_latest_block_header_timestamp_strict()`
  - `get_allow_black_hole_optimization_strict()` or `support_black_hole_optimization_strict()`
  - `get_create_new_account_bandwidth_rate_strict()`
  - `get_free_net_limit_strict()`
  - `get_create_account_fee_strict()`
  - `get_total_create_account_cost_strict()`
- Reuse `get_allow_multi_sign()` for the strict path because it already errors when missing.
- If the code starts repeating the same pattern, add a small internal helper for “read big-endian i64/u64 from dynamic-properties DB or return `not found <KEY>`”.

Acceptance criteria:

- Strict getters return an error when the key is absent.
- Error messages are stable and intentionally Java-aligned where practical.
- Existing non-strict getters keep current fallback behavior.

### Phase 2: Wire strict mode into account-create execution

Target file:

- `rust-backend/crates/core/src/service/mod.rs`

Work:

- In `execute_account_create_contract()`, read `self.get_execution_config()?.remote.strict_dynamic_properties`.
- Route dynamic-property access through the strict getters when the flag is enabled.
- Keep the current fallback getters when the flag is disabled.
- Cover both the always-touched path and the tracked-bandwidth path.
- Preserve current success-path behavior for already-covered fixtures.

Recommended structure:

- Introduce a tiny local helper or struct for account-create dynamic-property reads, so strict/non-strict branching is centralized instead of repeated at each call site.

Acceptance criteria:

- Strict mode fails fast on missing keys used by the path.
- Non-strict mode remains backward-compatible.
- Existing ordinary account-create tests continue to pass.

### Phase 3: Add focused Rust unit tests for missing-key behavior

Target file:

- `rust-backend/crates/core/src/service/tests/contracts/account_create.rs`

Add tests for strict mode:

- missing `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- missing `LATEST_BLOCK_HEADER_TIMESTAMP`
- missing `ALLOW_MULTI_SIGN`
- missing `ALLOW_BLACKHOLE_OPTIMIZATION`

Add tracked-path strict tests:

- missing `FREE_NET_LIMIT`
- missing `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
- missing `CREATE_ACCOUNT_FEE` when the fee fallback path is forced
- missing `TOTAL_CREATE_ACCOUNT_COST` when the fee fallback path is forced

Add regression controls:

- with `strict_dynamic_properties=false`, the same missing keys still follow legacy fallback behavior
- existing happy-path/edge-path account-create tests stay green

Acceptance criteria:

- Strict-mode failures are covered by tests for each missing key.
- Legacy mode is explicitly tested and preserved.

### Phase 4: Add Java fixture generation for missing-key cases

Primary target:

- `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`

Work:

- Add dedicated `ACCOUNT_CREATE_CONTRACT` fixture generators that deliberately remove dynamic-property keys after common initialization.
- Use distinct case names such as:
  - `validate_fail_missing_create_new_account_fee_in_system_contract`
  - `validate_fail_missing_latest_block_header_timestamp`
  - `validate_fail_missing_allow_multi_sign`
  - `validate_fail_missing_allow_blackhole_optimization`
- If tracked-bandwidth strict coverage is desired in conformance, add cases for:
  - `validate_fail_missing_free_net_limit`
  - `validate_fail_missing_create_new_account_bandwidth_rate`
  - `validate_fail_missing_create_account_fee`
  - `validate_fail_missing_total_create_account_cost`

Fixture metadata updates:

- Mark these fixtures as strict-dynamic-property fixtures.
- Prefer an explicit metadata field instead of encoding the behavior only in the case name.

Recommended metadata extension:

- Add a boolean like `strictDynamicProperties` to fixture metadata/schema.
- Optionally add another explicit switch if some cases require tracked AEXT mode.

Acceptance criteria:

- Missing-key fixtures are generated under `conformance/fixtures/account_create_contract/`.
- Fixture metadata clearly tells the runner to enable strict mode.
- Expected failure output reflects Java behavior, not Rust fallback behavior.

### Phase 5: Teach the conformance runner to honor strict fixtures

Target files:

- `rust-backend/crates/core/src/conformance/metadata.rs`
- `rust-backend/crates/core/src/conformance/runner.rs`
- `conformance/schema/metadata_schema.json`
- optionally `conformance/README.md`

Work:

- Extend fixture metadata parsing to carry a strict-dynamic-properties flag.
- Update `ConformanceRunner::create_conformance_config()` to accept fixture-specific overrides instead of using only one static config.
- For fixtures marked strict:
  - set `strict_dynamic_properties=true`
- For fixtures that need tracked-bandwidth-path coverage:
  - set `accountinfo_aext_mode="tracked"`

Recommended approach:

- Replace the current static `create_conformance_config()` helper with a metadata-aware config builder.
- Keep the default runner behavior unchanged for all non-strict fixtures.

Acceptance criteria:

- Existing fixtures still run with current behavior.
- New strict account-create fixtures execute with strict mode enabled.
- The runner can express strict account-create cases without accidentally changing unrelated fixture families.

### Phase 6: Add Java-side account-create remote validation

New tests to add:

- `framework/src/test/java/org/tron/core/execution/spi/RemoteExecutionSPIAccountCreateTest.java`

What to verify:

- `RemoteExecutionSPI` maps `AccountCreateContract` to:
  - `TxKind.NON_VM`
  - `ACCOUNT_CREATE_CONTRACT`
  - `data = full AccountCreateContract proto bytes`
  - `fromAddress = owner_address`
  - `toAddress = empty`

Add focused remote-vs-embedded validation:

- Prefer a dedicated account-create remote execution integration test instead of extending the generic `DualStorageModeIntegrationTest`.
- If you do touch dual-mode tests, use them only as supplementary storage/backend smoke coverage; they are not enough on their own.

Acceptance criteria:

- Java-side request mapping for account-create is explicitly locked by a test.
- There is at least one focused Java validation that exercises remote account-create parity beyond storage-factory wiring.

### Phase 7: Regenerate fixtures and validate end-to-end

Run sequence:

1. Rust unit tests for account-create
2. Java fixture generator for `CoreAccountFixtureGeneratorTest`
3. Rust conformance runner against `account_create_contract` fixtures
4. Focused Java remote-validation tests

Commands:

- `cd rust-backend && cargo test --package tron-backend-core account_create -- --nocapture`
- `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=./conformance/fixtures --dependency-verification=off`
- `CONFORMANCE_FIXTURES_DIR=./conformance/fixtures cargo test --package tron-backend-core conformance -- --nocapture --ignored`
- Focused Java remote-validation command once the new test class exists

Acceptance criteria:

- Missing-key account-create fixtures exist and pass in Rust conformance.
- Ordinary account-create fixtures remain green.
- Java-side request mapping and remote-validation tests pass.

## Suggested execution order

Use this order to avoid churn:

1. Rust strict getters
2. Rust service wiring
3. Rust unit tests
4. Java fixture generation updates
5. Metadata/schema updates
6. Conformance runner config wiring
7. Java request-mapping test
8. Focused remote-validation test
9. Full verification run

## Risks and mitigations

Risk: enabling strict mode globally in the runner could break unrelated fixture families.

Mitigation:

- make strictness fixture-driven, not global

Risk: tracked-bandwidth missing-key cases will not execute if the runner stays in `accountinfo_aext_mode="none"`.

Mitigation:

- allow metadata-driven runner overrides for AEXT mode

Risk: Java fixture generation may accidentally re-seed deleted dynamic properties before the contract executes.

Mitigation:

- remove the key immediately before `generator.generate(...)`
- assert the fixture result captures the intended missing-key failure

Risk: error strings may differ slightly between Java and Rust for some missing-key branches.

Mitigation:

- lock exact strings for the keys where Java’s message is stable
- otherwise assert on precise substrings and document the chosen parity boundary

## Definition of done

- `ACCOUNT_CREATE_CONTRACT` honors `strict_dynamic_properties=true` for all dynamic properties it actually uses.
- Rust has account-create missing-key tests for strict and non-strict modes.
- Java fixture generation includes missing-key account-create cases.
- The Rust conformance runner can execute those fixtures in strict mode.
- Java has explicit account-create remote request-mapping coverage and focused remote-validation coverage.
