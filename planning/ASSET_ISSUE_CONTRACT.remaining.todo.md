# Remaining TODO / Checklist: `ASSET_ISSUE_CONTRACT`

## Branch Status

- [x] Confirm Rust now emits `Trc10Change::AssetIssued.token_id`.
- [x] Confirm Java code paths already prefer the provided `token_id` when present.
- [x] Confirm emitted-address serialization still hardcodes `0x41` in the Rust gRPC conversion layer.
- [x] Confirm there is no targeted Java test yet for the non-empty `token_id` path in asset issuance.
- [x] Confirm there is no targeted non-mainnet emitted-address regression test yet.

## 1. Prefix Plumbing Decision

- [x] Decide how the response conversion layer receives the effective DB prefix.
- [x] Prefer passing the prefix into `convert_execution_result_to_protobuf(...)` from the gRPC execution path.
- [x] Document the chosen prefix source in code comments so future contracts do not reintroduce `0x41` assumptions.

## 2. Rust Serialization Fix

- [x] Update `rust-backend/crates/core/src/service/grpc/conversion.rs` so emitted addresses use `add_tron_address_prefix_with(..., prefix)` instead of the hardcoded helper.
- [x] Fix `Trc10AssetIssued.owner_address` serialization.
- [x] Fix `Trc10AssetTransferred.owner_address` serialization.
- [x] Fix `Trc10AssetTransferred.to_address` serialization.
- [x] Audit every other emitted-address call site in `conversion.rs`.
- [x] Decide whether the audit result requires fixing logs, state changes, freeze changes, vote changes, withdraw changes, AEXT/account snapshots, and contract-address emission in the same patch.
- [x] Keep request-side address stripping/validation behavior unchanged unless the audit shows a directly related bug.

## 3. Rust Tests

- [x] Add a gRPC/result-conversion regression test for `Trc10AssetIssued.owner_address` with DB prefix `0xa0`.
- [x] Add a gRPC/result-conversion regression test for `Trc10AssetTransferred.owner_address` with DB prefix `0xa0`.
- [x] Add a gRPC/result-conversion regression test for `Trc10AssetTransferred.to_address` with DB prefix `0xa0`.
- [x] Add a mainnet-control test proving `0x41` behavior still works after the change.
- [x] If Phase 2 changes more address families, add at least one regression assertion for each changed family.

## 4. Java Runtime Verification

- [x] Add a `RuntimeSpiImplTest` case where `ExecutionSPI.Trc10AssetIssued` carries a non-empty `tokenId`.
- [x] Assert that `applyAssetIssuedChange(...)` uses the provided `tokenId` for the V2 store key.
- [x] Assert that the issuer account `assetV2` map uses the provided `tokenId`.
- [x] Assert that `TOKEN_ID_NUM` is not incremented again when the provided `tokenId` is non-empty.
- [x] Keep a separate fallback-path test for the empty-tokenId behavior so both branches stay covered.

## 5. Java Reporting Verification

- [x] Add a test for `ExecutionCsvRecordBuilder` that feeds an `AssetIssued` change with a non-empty `tokenId`.
- [x] Set `DynamicPropertiesStore.getTokenIdNum()` to a different value inside that test so fallback usage is detectable.
- [x] Assert that the emitted issuance-domain rows use the provided `tokenId`, not the dynamic-store value.
- [x] If needed, add a `RemoteExecutionSPI` parsing test to prove protobuf `token_id` survives the Rust-to-Java boundary intact.

## 6. Conformance / Validation Run

- [x] Run targeted Rust tests for the new conversion coverage.
  - `cargo test -p tron-backend-core --lib service::grpc::conversion::tests` — 8/8 passed
- [x] Run targeted Java runtime/reporting tests for the new `token_id` assertions.
  - `RuntimeSpiImplTest` — 13/13 passed (including new testTrc10AssetIssuedWithProvidedTokenIdSkipsIncrement, testTrc10AssetIssuedFallbackIncrementTokenIdNum)
  - `ExecutionCsvRecordBuilderTest` — 2/2 passed (testExtractTrc10DomainsUsesProvidedTokenId, testExtractTrc10DomainsEmptyTokenIdWithNullTrace)
- [x] Run `./gradlew :framework:test --tests "org.tron.core.conformance.AssetIssueFixtureGeneratorTest" -Dconformance.output=conformance/fixtures --dependency-verification=off`.
  - All fixture generation tests passed
- [x] Run the Rust conformance runner against the updated fixture set.
  - `scripts/ci/run_fixture_conformance.sh --rust-only --contract asset_issue`
  - 49/50 ASSET_ISSUE_CONTRACT fixtures passed; 1 pre-existing failure (happy_path_start_time_just_after_head_block_time: insufficient balance in genesis setup — not related to address prefix or token_id changes)
- [x] Record the exact commands and pass/fail results in the commit message, PR, or follow-up status note.

## 7. Closeout

- [x] Re-check the old note in `planning/review_again/ASSET_ISSUE_CONTRACT.todo.md`.
- [x] Replace or delete the stale note only after code, tests, and validation runs are complete.
  - Updated old note with all previously-unchecked items now marked done with evidence.
- [x] Include concrete evidence when closing it:
- [x] which Rust test covers non-mainnet prefix emission
  - `test_convert_result_trc10_issued_uses_testnet_prefix` — verifies `0xa0` prefix on `Trc10AssetIssued.owner_address`
  - `test_convert_result_trc10_transferred_uses_testnet_prefix` — verifies `0xa0` on both `owner_address` and `to_address`
  - `test_convert_result_logs_use_address_prefix` — verifies `0xa0` on log addresses
  - `test_convert_result_trc10_issued_uses_mainnet_prefix` — control test for `0x41`
- [x] which Java test proves direct `token_id` consumption
  - `RuntimeSpiImplTest.testTrc10AssetIssuedWithProvidedTokenIdSkipsIncrement` — verifies provided tokenId "1000042" is used as V2 store key, TOKEN_ID_NUM is not incremented, issuer assetV2 map uses provided ID
- [x] which Java/reporting test proves fallback is not used
  - `ExecutionCsvRecordBuilderTest.testExtractTrc10DomainsUsesProvidedTokenId` — sets TOKEN_ID_NUM to 9999999, provides tokenId "1000042", asserts issuance JSON contains "1000042" and not "9999999"
- [x] which conformance/fixture run passed
  - `scripts/ci/run_fixture_conformance.sh --rust-only --contract asset_issue` — 49/50 ASSET_ISSUE_CONTRACT passed (1 pre-existing failure unrelated to this work)
