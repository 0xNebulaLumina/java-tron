# Remaining TODO / Checklist: `ASSET_ISSUE_CONTRACT`

## Branch Status

- [x] Confirm Rust now emits `Trc10Change::AssetIssued.token_id`.
- [x] Confirm Java code paths already prefer the provided `token_id` when present.
- [x] Confirm emitted-address serialization still hardcodes `0x41` in the Rust gRPC conversion layer.
- [x] Confirm there is no targeted Java test yet for the non-empty `token_id` path in asset issuance.
- [x] Confirm there is no targeted non-mainnet emitted-address regression test yet.

## 1. Prefix Plumbing Decision

- [ ] Decide how the response conversion layer receives the effective DB prefix.
- [ ] Prefer passing the prefix into `convert_execution_result_to_protobuf(...)` from the gRPC execution path.
- [ ] Document the chosen prefix source in code comments so future contracts do not reintroduce `0x41` assumptions.

## 2. Rust Serialization Fix

- [ ] Update `rust-backend/crates/core/src/service/grpc/conversion.rs` so emitted addresses use `add_tron_address_prefix_with(..., prefix)` instead of the hardcoded helper.
- [ ] Fix `Trc10AssetIssued.owner_address` serialization.
- [ ] Fix `Trc10AssetTransferred.owner_address` serialization.
- [ ] Fix `Trc10AssetTransferred.to_address` serialization.
- [ ] Audit every other emitted-address call site in `conversion.rs`.
- [ ] Decide whether the audit result requires fixing logs, state changes, freeze changes, vote changes, withdraw changes, AEXT/account snapshots, and contract-address emission in the same patch.
- [ ] Keep request-side address stripping/validation behavior unchanged unless the audit shows a directly related bug.

## 3. Rust Tests

- [ ] Add a gRPC/result-conversion regression test for `Trc10AssetIssued.owner_address` with DB prefix `0xa0`.
- [ ] Add a gRPC/result-conversion regression test for `Trc10AssetTransferred.owner_address` with DB prefix `0xa0`.
- [ ] Add a gRPC/result-conversion regression test for `Trc10AssetTransferred.to_address` with DB prefix `0xa0`.
- [ ] Add a mainnet-control test proving `0x41` behavior still works after the change.
- [ ] If Phase 2 changes more address families, add at least one regression assertion for each changed family.

## 4. Java Runtime Verification

- [ ] Add a `RuntimeSpiImplTest` case where `ExecutionSPI.Trc10AssetIssued` carries a non-empty `tokenId`.
- [ ] Assert that `applyAssetIssuedChange(...)` uses the provided `tokenId` for the V2 store key.
- [ ] Assert that the issuer account `assetV2` map uses the provided `tokenId`.
- [ ] Assert that `TOKEN_ID_NUM` is not incremented again when the provided `tokenId` is non-empty.
- [ ] Keep a separate fallback-path test for the empty-tokenId behavior so both branches stay covered.

## 5. Java Reporting Verification

- [ ] Add a test for `ExecutionCsvRecordBuilder` that feeds an `AssetIssued` change with a non-empty `tokenId`.
- [ ] Set `DynamicPropertiesStore.getTokenIdNum()` to a different value inside that test so fallback usage is detectable.
- [ ] Assert that the emitted issuance-domain rows use the provided `tokenId`, not the dynamic-store value.
- [ ] If needed, add a `RemoteExecutionSPI` parsing test to prove protobuf `token_id` survives the Rust-to-Java boundary intact.

## 6. Conformance / Validation Run

- [ ] Run targeted Rust tests for the new conversion coverage.
- [ ] Run targeted Java runtime/reporting tests for the new `token_id` assertions.
- [ ] Run `./gradlew :framework:test --tests "org.tron.core.conformance.AssetIssueFixtureGeneratorTest" -Dconformance.output=conformance/fixtures --dependency-verification=off`.
- [ ] Run the Rust conformance runner against the updated fixture set.
- [ ] Record the exact commands and pass/fail results in the commit message, PR, or follow-up status note.

## 7. Closeout

- [ ] Re-check the old note in `planning/review_again/ASSET_ISSUE_CONTRACT.todo.md`.
- [ ] Replace or delete the stale note only after code, tests, and validation runs are complete.
- [ ] Include concrete evidence when closing it:
- [ ] which Rust test covers non-mainnet prefix emission
- [ ] which Java test proves direct `token_id` consumption
- [ ] which Java/reporting test proves fallback is not used
- [ ] which conformance/fixture run passed
