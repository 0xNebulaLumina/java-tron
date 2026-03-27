# TODO: ACCOUNT_CREATE_CONTRACT missing-dynamic-property parity

## 0. Baseline and scope

- [ ] Keep current non-strict behavior unchanged when `execution.remote.strict_dynamic_properties=false`
- [ ] Implement strict missing-key parity only for keys actually touched by `ACCOUNT_CREATE_CONTRACT`
- [ ] Treat `ACTIVE_DEFAULT_OPERATIONS` as defaulted, not strict-fail, because Java also defaults it

## 1. Rust dynamic-property layer

- [x] Update `rust-backend/crates/execution/src/storage_adapter/engine.rs`
- [x] Add `get_create_new_account_fee_in_system_contract_strict()`
- [x] Add `get_latest_block_header_timestamp_strict()`
- [x] Add `get_allow_black_hole_optimization_strict()` or `support_black_hole_optimization_strict()`
- [x] Add `get_create_new_account_bandwidth_rate_strict()`
- [x] Add `get_free_net_limit_strict()`
- [x] Add `get_create_account_fee_strict()`
- [x] Add `get_total_create_account_cost_strict()`
- [x] Reuse existing strict `get_allow_multi_sign()` instead of duplicating it
- [x] If repetition gets high, add one internal helper for strict dynamic-property decoding/error construction

## 2. Rust account-create service wiring

- [x] Update `rust-backend/crates/core/src/service/mod.rs`
- [x] Read `self.get_execution_config()?.remote.strict_dynamic_properties` inside `execute_account_create_contract()`
- [x] Route actuator-fee reads through strict getters when strict mode is on:
  - [x] `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
  - [x] `LATEST_BLOCK_HEADER_TIMESTAMP`
  - [x] `ALLOW_MULTI_SIGN`
  - [x] `ALLOW_BLACKHOLE_OPTIMIZATION`
- [x] Route tracked-bandwidth reads through strict getters when strict mode is on:
  - [x] `FREE_NET_LIMIT`
  - [x] `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
  - [x] `CREATE_ACCOUNT_FEE`
  - [x] `TOTAL_CREATE_ACCOUNT_COST`
- [x] Keep current fallback getters when strict mode is off
- [x] Preserve current happy-path behavior for existing fixtures and tests

## 3. Rust unit tests

- [x] Update `rust-backend/crates/core/src/service/tests/contracts/account_create.rs`
- [x] Add helper to build a test service with:
  - [x] `account_create_enabled=true`
  - [x] `strict_dynamic_properties=true`
- [x] Add strict missing-key test for `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
- [x] Add strict missing-key test for `LATEST_BLOCK_HEADER_TIMESTAMP`
- [x] Add strict missing-key test for `ALLOW_MULTI_SIGN`
- [x] Add strict missing-key test for `ALLOW_BLACKHOLE_OPTIMIZATION`
- [x] Add tracked-path strict missing-key test for `FREE_NET_LIMIT`
- [x] Add tracked-path strict missing-key test for `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
- [x] Add tracked-path strict missing-key test for `CREATE_ACCOUNT_FEE`
- [x] Add tracked-path strict missing-key test for `TOTAL_CREATE_ACCOUNT_COST`
- [x] Add control tests proving fallback behavior still works when strict mode is off
- [x] Re-run the existing account-create parity tests after the new strict tests are added

## 4. Java fixture generation

- [x] Update `framework/src/test/java/org/tron/core/conformance/CoreAccountFixtureGeneratorTest.java`
- [x] Add fixture: `validate_fail_missing_create_new_account_fee_in_system_contract`
- [x] Add fixture: `validate_fail_missing_latest_block_header_timestamp`
- [x] Add fixture: `validate_fail_missing_allow_multi_sign`
- [x] Add fixture: `validate_fail_missing_allow_blackhole_optimization`
- [x] If tracked-bandwidth strict coverage is desired, add fixtures for:
  - [x] `validate_fail_missing_free_net_limit`
  - [x] `validate_fail_missing_create_new_account_bandwidth_rate`
  - [x] `validate_fail_missing_create_account_fee`
  - [x] `validate_fail_missing_total_create_account_cost`
- [x] Make sure each fixture removes the target key after common initialization and before `generator.generate(...)`
- [x] Assert each new fixture really fails on the intended missing-key branch

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
