# Remaining Plan: `ASSET_ISSUE_CONTRACT`

## Current Branch Assessment

This branch has already closed part of the old gap, but not all of it.

What is already implemented:

- Rust now emits `token_id` for `Trc10Change::AssetIssued` in `rust-backend/crates/core/src/service/mod.rs`.
- Java already has direct-consume paths for that field:
  - `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` forwards `protoAssetIssued.getTokenId()`.
  - `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` uses the provided `tokenId` and only falls back to `DynamicPropertiesStore.getTokenIdNum()` when it is empty.
  - `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java` also prefers the provided `tokenId`.

What is still unfinished or still unverified:

1. DB-prefix emission is still unfinished.
   - `rust-backend/crates/core/src/service/grpc/address.rs` keeps `add_tron_address_prefix()` as a hardcoded `0x41` helper.
   - `rust-backend/crates/core/src/service/grpc/conversion.rs` still uses that hardcoded helper when serializing emitted addresses back to Java, including:
     - `Trc10AssetIssued.owner_address`
     - `Trc10AssetTransferred.owner_address`
     - `Trc10AssetTransferred.to_address`
   - That means a non-mainnet DB prefix can still be lost at the protobuf/gRPC boundary even though contract-side validation already uses the DB prefix.

2. Java consumption of Rust-emitted `token_id` is implemented but not actually locked by tests.
   - Existing `RuntimeSpiImplTest` asset-issue cases still construct `Trc10AssetIssued` with an empty token ID and therefore only exercise the fallback path.
   - I did not find a targeted Java test proving that a non-empty Rust-provided `token_id` is preserved, used for store writes, and does not trigger another `TOKEN_ID_NUM` increment.
   - I also did not find a targeted CSV/domain test proving `ExecutionCsvRecordBuilder` uses the provided `token_id` when it disagrees with or is independent from `DynamicPropertiesStore`.

3. Asset-issue-specific conformance/Java verification is still incomplete as a recorded work item.
   - The branch already contains `AssetIssueFixtureGeneratorTest` and checked-in fixtures under `conformance/fixtures/asset_issue_contract/`.
   - The Rust conformance runner exists.
   - But there is no checked-in proof that the remaining gap was validated end-to-end after the recent `token_id` and prefix changes, and no targeted coverage for the non-mainnet emitted-address case.

## Objective

Finish the remaining work without changing semantics outside the actual parity gap:

- make all emitted addresses use the DB/configured prefix, not a hardcoded mainnet prefix;
- prove that Java consumes the Rust-emitted `token_id` directly;
- run the missing Java/conformance validation so the old note can be removed with evidence.

## Phase 1: Lock Scope And Prefix Source

Decide the exact prefix source used during result serialization.

Recommended approach:

- treat the DB/storage prefix as the single source of truth;
- thread that prefix into `convert_execution_result_to_protobuf(...)`;
- replace hardcoded `add_tron_address_prefix(...)` calls with `add_tron_address_prefix_with(..., prefix)` for every emitted address that leaves Rust and returns to Java.

Design constraints:

- do not change the internal execution model; `Trc10AssetIssued.owner_address` can remain a 20-byte EVM address internally;
- only change the serialization boundary;
- avoid mixing request-time prefix rules with response-time prefix rules.

Open implementation detail to resolve before coding:

- whether the prefix should be passed into `convert_execution_result_to_protobuf(...)` from the current storage adapter call site, or stored inside `TronExecutionResult` during execution.

Preferred choice:

- pass the resolved prefix from the gRPC execution path into the conversion function, because the issue is a response-serialization concern, not core execution state.

## Phase 2: Fix Result Serialization

Update the Rust response conversion layer.

Primary file:

- `rust-backend/crates/core/src/service/grpc/conversion.rs`

Supporting helper:

- `rust-backend/crates/core/src/service/grpc/address.rs`

Required work:

- update the conversion function signature so it has access to the resolved DB prefix;
- replace hardcoded prefix attachment for:
  - `Trc10AssetIssued.owner_address`
  - `Trc10AssetTransferred.owner_address`
  - `Trc10AssetTransferred.to_address`
- audit the rest of `conversion.rs` and decide whether the same fix must be applied to other emitted address families in the same function:
  - logs
  - state changes
  - freeze changes
  - vote changes
  - withdraw changes
  - contract addresses
  - AEXT/account snapshots

Because the old note says "all emitted addresses", the audit should not stop at asset-issue-only fields once the prefix plumbing is in place.

## Phase 3: Add Rust Regression Tests

Add tests that fail on the current branch and prove the serialization fix.

Minimum required Rust coverage:

- a test where the effective prefix is `0xa0` and serialized `Trc10AssetIssued.owner_address` comes back with `0xa0`;
- a test where the effective prefix is `0xa0` and serialized `Trc10AssetTransferred.owner_address` and `to_address` come back with `0xa0`;
- a control test showing `0x41` behavior remains unchanged for mainnet-style storage;
- if the audit in Phase 2 touches other emitted address families, add one shared conversion test covering those call sites too.

Best place to add coverage:

- near the gRPC conversion tests, not only the contract execution tests, because the current gap lives in result serialization, not inside `execute_asset_issue_contract()`.

## Phase 4: Add Java Verification For Provided `token_id`

Lock the direct-consume behavior with targeted Java tests.

Primary files:

- `framework/src/test/java/org/tron/common/runtime/RuntimeSpiImplTest.java`
- `framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder` tests, or a new focused test file if no suitable one exists
- optionally `framework/src/test/java/org/tron/core/execution/spi/RemoteExecutionSPI` tests if protobuf parsing coverage is missing

Required assertions:

1. `RuntimeSpiImpl.applyAssetIssuedChange(...)`
   - when `assetIssued.getTokenId()` is non-empty, Java must use that exact ID;
   - `TOKEN_ID_NUM` must not be incremented again in that path;
   - V2 store and issuer `assetV2` map must use the provided ID;
   - legacy V1 behavior must still depend only on `ALLOW_SAME_TOKEN_NAME`.

2. `ExecutionCsvRecordBuilder.extractTrc10Domains(...)`
   - when `Trc10AssetIssued.tokenId` is non-empty, CSV/domain output must use it;
   - the test should intentionally set `DynamicPropertiesStore.getTokenIdNum()` to a different value so the fallback path would be detectable if used by mistake.

3. `RemoteExecutionSPI`
   - if coverage is missing, add a decoding test proving protobuf `Trc10AssetIssued.token_id` is preserved when converting the Rust response into Java-side `ExecutionSPI.Trc10AssetIssued`.

## Phase 5: Run Asset-Issue Validation

After code and tests are in place, run the verification that the old note says is still missing.

Java tests to run:

- `./gradlew :framework:test --tests "org.tron.common.runtime.RuntimeSpiImplTest" --dependency-verification=off`
- targeted CSV/reporting test class once added
- `./gradlew :framework:test --tests "org.tron.core.conformance.AssetIssueFixtureGeneratorTest" -Dconformance.output=conformance/fixtures --dependency-verification=off`

Rust tests to run:

- targeted gRPC conversion test(s)
- targeted asset-issue Rust tests if any execution-facing assertions are touched

Conformance run:

- run the Rust fixture conformance runner against the generated fixture set;
- if the existing script is used, note that `scripts/ci/run_fixture_conformance.sh --contract ...` does not currently expose an `asset_issue` filter, so use a direct invocation or run the broader fixture suite.

## Exit Criteria

The old note can be deleted only after all of the following are true:

- emitted protobuf/gRPC addresses use the DB prefix rather than a hardcoded `0x41`;
- there is a targeted Java test proving non-empty Rust-emitted `token_id` is consumed directly;
- there is a targeted reporting/CSV test proving non-empty `token_id` is preferred over the fallback store lookup;
- Rust regression tests cover the non-mainnet prefix path;
- Java fixture generation and Rust conformance validation have both been run and recorded as passing.
