# TODO: ACCOUNT_CREATE_CONTRACT missing-dynamic-property parity

## 0. Baseline and scope

- [ ] Keep current non-strict behavior unchanged when `execution.remote.strict_dynamic_properties=false`
- [ ] Implement strict missing-key parity only for keys actually touched by `ACCOUNT_CREATE_CONTRACT`
- [ ] Treat `ACTIVE_DEFAULT_OPERATIONS` as defaulted, not strict-fail, because Java also defaults it

## 1. Rust dynamic-property layer

- [ ] Update `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- [ ] Add `get_create_new_account_fee_in_system_contract_strict()`
- [ ] Add `get_latest_block_header_timestamp_strict()`
- [ ] Add `get_allow_black_hole_optimization_strict()` or `support_black_hole_optimization_strict()`
- [ ] Add `get_create_new_account_bandwidth_rate_strict()`
- [ ] Add `get_free_net_limit_strict()`
- [ ] Add `get_create_account_fee_strict()`
- [ ] Add `get_total_create_account_cost_strict()`
- [ ] Reuse existing strict `get_allow_multi_sign()` instead of duplicating it
- [ ] If repetition gets high, add one internal helper for strict dynamic-property decoding/error construction

## 2. Rust account-create service wiring

- [ ] Update `rust-backend/crates/core/src/service/mod.rs`
- [ ] Read `self.get_execution_config()?.remote.strict_dynamic_properties` inside `execute_account_create_contract()`
- [ ] Route actuator-fee reads through strict getters when strict mode is on:
  - [ ] `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
  - [ ] `LATEST_BLOCK_HEADER_TIMESTAMP`
  - [ ] `ALLOW_MULTI_SIGN`
  - [ ] `ALLOW_BLACKHOLE_OPTIMIZATION`
- [ ] Route tracked-bandwidth reads through strict getters when strict mode is on:
  - [ ] `FREE_NET_LIMIT`
  - [ ] `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
  - [ ] `CREATE_ACCOUNT_FEE`
  - [ ] `TOTAL_CREATE_ACCOUNT_COST`
- [ ] Keep current fallback getters when strict mode is off
- [ ] Preserve current happy-path behavior for existing fixtures and tests

## 3. Rust unit tests

- [ ] Update `rust-backend/crates/core/src/service/tests/contracts/account_create.rs`
- [ ] Add helper to build a test service with:
  - [ ] `account_create_enabled=true`
  - [ ] `strict_dynamic_properties=true`
- [ ] Add strict missing-key test for `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- [ ] Add strict missing-key test for `LATEST_BLOCK_HEADER_TIMESTAMP`
- [ ] Add strict missing-key test for `ALLOW_MULTI_SIGN`
- [ ] Add strict missing-key test for `ALLOW_BLACKHOLE_OPTIMIZATION`
- [ ] Add tracked-path strict missing-key test for `FREE_NET_LIMIT`
- [ ] Add tracked-path strict missing-key test for `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
- [ ] Add tracked-path strict missing-key test for `CREATE_ACCOUNT_FEE`
- [ ] Add tracked-path strict missing-key test for `TOTAL_CREATE_ACCOUNT_COST`
- [ ] Add control tests proving fallback behavior still works when strict mode is off
- [ ] Re-run the existing account-create parity tests after the new strict tests are added

## 4. Java fixture generation

- [ ] Update `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`
- [ ] Add fixture: `validate_fail_missing_create_new_account_fee_in_system_contract`
- [ ] Add fixture: `validate_fail_missing_latest_block_header_timestamp`
- [ ] Add fixture: `validate_fail_missing_allow_multi_sign`
- [ ] Add fixture: `validate_fail_missing_allow_blackhole_optimization`
- [ ] If tracked-bandwidth strict coverage is desired, add fixtures for:
  - [ ] `validate_fail_missing_free_net_limit`
  - [ ] `validate_fail_missing_create_new_account_bandwidth_rate`
  - [ ] `validate_fail_missing_create_account_fee`
  - [ ] `validate_fail_missing_total_create_account_cost`
- [ ] Make sure each fixture removes the target key after common initialization and before `generator.generate(...)`
- [ ] Assert each new fixture really fails on the intended missing-key branch

## 5. Fixture metadata and schema

- [ ] Update fixture metadata model to carry strict-dynamic-property intent
- [ ] Update `framework/src/test/java/org/tron/core/conformance/FixtureMetadata.java`
- [ ] Update `rust-backend/crates/core/src/conformance/metadata.rs`
- [ ] Update `conformance/schema/metadata_schema.json`
- [ ] Decide whether metadata also needs an `accountinfo_aext_mode` override for tracked-path fixtures
- [ ] Update `conformance/README.md` if new metadata fields are added

## 6. Conformance runner

- [ ] Update `rust-backend/crates/core/src/conformance/runner.rs`
- [ ] Replace the static `create_conformance_config()` with a metadata-aware config builder
- [ ] When fixture metadata requests strict mode, set `strict_dynamic_properties=true`
- [ ] When fixture metadata requests tracked-bandwidth semantics, set `accountinfo_aext_mode="tracked"`
- [ ] Keep non-strict fixtures on the current default behavior
- [ ] Verify account-create missing-key fixtures are not filtered out by `scripts/ci/run_fixture_conformance.sh`

## 7. Java remote-execution validation

- [ ] Add `framework/src/test/java/org/tron/core/execution/spi/RemoteExecutionSPIAccountCreateTest.java`
- [ ] Assert `AccountCreateContract` maps to `TxKind.NON_VM`
- [ ] Assert contract type maps to `ACCOUNT_CREATE_CONTRACT`
- [ ] Assert `fromAddress` is the owner address
- [ ] Assert `toAddress` is empty
- [ ] Assert `data` contains the full serialized `AccountCreateContract`
- [ ] Add one focused Java remote-vs-embedded account-create validation test
- [ ] Do not rely on `DualStorageModeIntegrationTest` alone as proof of account-create parity

## 8. Fixture regeneration and verification

- [ ] Regenerate account fixtures:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.CoreAccountFixtureGeneratorTest" -Dconformance.output=./conformance/fixtures --dependency-verification=off`
- [ ] Check the new fixture directories exist under `conformance/fixtures/account_create_contract/`
- [ ] Run Rust account-create tests:
  - [ ] `cd rust-backend && cargo test --package tron-backend-core account_create -- --nocapture`
- [ ] Run Rust conformance against regenerated fixtures:
  - [ ] `cd rust-backend && CONFORMANCE_FIXTURES_DIR=../conformance/fixtures cargo test --package tron-backend-core conformance -- --nocapture --ignored`
- [ ] Run the new Java `RemoteExecutionSPIAccountCreateTest`
- [ ] Run the focused Java remote-validation test

## 9. Close-out checks

- [ ] Confirm ordinary `account_create_contract` fixtures still pass
- [ ] Confirm strict missing-key fixtures fail for the same reasons as Java
- [ ] Confirm strict mode is fixture-driven in conformance and does not silently change unrelated fixture families
- [ ] Confirm no accidental behavior change when `strict_dynamic_properties=false`
