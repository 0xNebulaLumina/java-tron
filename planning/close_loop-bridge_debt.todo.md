# Todo List

## Locked decisions

- B2 removal means deleting Java-side state application from `RuntimeSpiImpl`; Rust remains the canonical `RR` writer and Java only runs B3 `postExecMirror(...)` for read-side refresh.
- Successful remote `COMPUTE_ONLY` execution is no longer supported. A successful non-`PERSISTED` result must fail fast.
- Effectful non-`PERSISTED` remote results are invalid after B2 removal. Only failed/reverted non-`PERSISTED` results with no remote effects may pass through.
- `RuntimeSpiImpl` block execution is restricted to `execution.mode=REMOTE`. `EMBEDDED` stays on `RuntimeImpl`, and `SHADOW` is not routed through this canonical runtime path.
- `hasPersistedMirrorEffects(...)` will be renamed to `hasRemoteStateEffects(...)` and retained as the shared remote-effect detector.
- B3 mirror, pre-state snapshot capture, `ExecutionSPI` sidecar model types, `RemoteExecutionSPI` sidecar conversion, and CSV/reporting code remain in place.
- Reflection tests that invoke private `apply*` methods will be replaced with runtime ownership and mirror-safety tests.
- `remote.exec.apply.*` JVM flags are removed from active runtime/script/config documentation because Java apply no longer exists.

## Implementation tasks

- [ ] Replace the `RuntimeSpiImpl.execute(...)` apply branch with persisted-mirror and fail-fast ownership handling
  - Change `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` around the current `writeMode` branch.
  - Keep `writeMode == ExecutionSPI.WriteMode.PERSISTED` on the `postExecMirror(executionResult, context)` path.
  - For `writeMode != PERSISTED`, throw `IllegalStateException` when `executionResult.isSuccess()` is true.
  - For `writeMode != PERSISTED`, throw `IllegalStateException` when `hasRemoteStateEffects(executionResult)` is true.
  - Allow only `writeMode != PERSISTED && !executionResult.isSuccess() && !hasRemoteStateEffects(executionResult)` to continue without mirror.
  - Definition of done: no `RuntimeSpiImpl.execute(...)` path calls any `apply*` method, and non-persisted effectful remote results cannot silently pass.

- [ ] Rename and retain the remote-effect detector
  - Rename `hasPersistedMirrorEffects(...)` to `hasRemoteStateEffects(...)` in `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`.
  - Preserve its checks for non-empty `stateChanges`, `freezeChanges`, `globalResourceChanges`, `trc10Changes`, `voteChanges`, `withdrawChanges`, and `contractAddress`.
  - Update the empty-touched-key guard inside `postExecMirror(...)` to call the renamed helper.
  - Definition of done: the helper name matches its broader use in both persisted mirror validation and non-persisted fail-fast validation.

- [ ] Delete the five top-level Java apply methods from `RuntimeSpiImpl`
  - Remove `applyStateChangesToLocalDatabase(...)`.
  - Remove `applyFreezeLedgerChanges(...)`.
  - Remove `applyTrc10Changes(...)`.
  - Remove `applyVoteChanges(...)`.
  - Remove `applyWithdrawChanges(...)`.
  - Definition of done: `grep -n "applyStateChangesToLocalDatabase\|applyFreezeLedgerChanges\|applyTrc10Changes\|applyVoteChanges\|applyWithdrawChanges" framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` returns no matches.

- [ ] Delete apply-only helper code from `RuntimeSpiImpl`
  - Remove `applyFreezeLedgerChange(...)`, `applyFreezeV1Change(...)`, `applyFreezeV2Change(...)`, and `applyGlobalResourceChange(...)`.
  - Remove `applyAssetIssuedChange(...)` and `applyAssetTransferredChange(...)`.
  - Remove `applyStateChange(...)`, `updateAccountState(...)`, and `updateAccountStorage(...)`.
  - Remove `bytesToLong(...)`, `bytesToLongFromBalance(...)`, nested `AccountInfo`, and `deserializeAccountInfo(...)`.
  - Definition of done: no helper remains whose only purpose was applying Rust-returned state into Java stores.

- [ ] Clean `RuntimeSpiImpl` imports after deleting apply code
  - Remove imports that become unused after deleting Java apply logic.
  - Keep imports used by B3 mirror and pre-state snapshot capture, including `AccountCapsule`, `ByteArrayWrapper`, `TronStoreWithRevoking`, `StorageSPI`, `StorageMode`, `StorageSpiFactory`, `PreStateSnapshotRegistry`, `ExecutionSPI.StateChange`, and `Vote` when still referenced.
  - Definition of done: `:framework:compileJava` has no unused-import compilation failures.

- [ ] Restrict `RuntimeSpiImpl` creation to remote execution mode
  - Update `framework/src/main/java/org/tron/core/db/Manager.java` in `shouldUseExecutionSpi()`.
  - Keep the existing `CommonParameter.getInstance().isExecutionSpiEnabled()` gate.
  - Add an explicit mode check so the method returns true only when `ExecutionSpiFactory.determineExecutionMode() == ExecutionMode.REMOTE`.
  - Ensure `EMBEDDED` and `SHADOW` return false from `shouldUseExecutionSpi()` and therefore use `RuntimeImpl` in `createRuntime()`.
  - Definition of done: block execution no longer routes `EMBEDDED` or `SHADOW` through `RuntimeSpiImpl`.

- [ ] Update sidecar comments so they no longer describe Java apply as active behavior
  - Update comments in `framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java` for `StateChange`, `FreezeLedgerChange`, `GlobalResourceTotalsChange`, `Trc10Change`, `VoteChange`, and `WithdrawChange` where they say Java applies these fields.
  - Update comments in `framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java` that describe sidecars as Java-side application data.
  - Keep the fields and accessors unchanged unless compilation requires import cleanup.
  - Definition of done: comments describe sidecars as reporting, pre-state snapshot, legacy compute-only metadata, or mirror-validation data, not active Java write instructions.

- [ ] Keep B3 mirror code intact
  - Preserve `postExecMirror(...)`, `normalizeTouchedKeys(...)`, `processBatchReads(...)`, `processPerKeyReads(...)`, `KeyOperation`, `getMirrorRemoteStorageSPI()`, and `getStoreByDbName(...)` in `RuntimeSpiImpl`.
  - Preserve batch-get flags `remote.exec.postexec.mirror.batchGet`, `remote.exec.postexec.mirror.batchGet.maxKeys`, and `remote.exec.postexec.mirror.batchGet.fallbackToSingleGet`.
  - Definition of done: persisted-mode execution still has a Java read-side refresh path through touched keys.

- [ ] Keep CSV and pre-state snapshot behavior intact
  - Preserve `capturePreStateSnapshot(executionResult, context)` before the write-mode branch in `RuntimeSpiImpl.execute(...)`.
  - Preserve `capturePreStateSnapshot(...)` and `isV1UnfreezeContract(...)` in `RuntimeSpiImpl`.
  - Preserve sidecar extraction in `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`.
  - Preserve CSV sidecar usage in `framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java`.
  - Definition of done: CSV/reporting code still receives remote sidecars and account-state bytes for digest generation.

- [ ] Replace `RuntimeSpiImplTest` private-apply coverage with ownership tests
  - Update `framework/src/test/java/org/tron/common/runtime/RuntimeSpiImplTest.java`.
  - Remove reflection calls to private `applyTrc10Changes(...)` at the current `RuntimeSpiImplTest.java:194-202`, `:442-449`, and `:515-521` regions.
  - Add test coverage for a successful `COMPUTE_ONLY` remote result causing `RuntimeSpiImpl.execute(...)` to fail fast.
  - Add test coverage for an effectful failed non-`PERSISTED` remote result causing `RuntimeSpiImpl.execute(...)` to fail fast.
  - Add test coverage for a failed non-`PERSISTED` remote result with no effects passing through without Java apply.
  - Use a minimal fake `ExecutionSPI` installed through the existing `ExecutionSpiFactory` singleton for these tests, restoring the original singleton in teardown.
  - Definition of done: tests validate ownership behavior instead of private Java apply methods.

- [ ] Add persisted mirror safety tests
  - Extend `framework/src/test/java/org/tron/common/runtime/RuntimeSpiImplTest.java` or the closest existing mirror-focused test.
  - Add coverage that `PERSISTED` with remote effects and an empty `touchedKeys` list fails before any remote storage read.
  - Add coverage that `PERSISTED` with touched keys enters the mirror path by injecting a fake `StorageSPI` into `RuntimeSpiImpl.mirrorRemoteStorageSPI` and verifying the local store is refreshed or deleted according to the touched key operation.
  - Keep existing touched-key normalization and batch-get coverage in `framework/src/test/java/org/tron/core/storage/spi/RemoteStorageBatchGetTest.java`.
  - Definition of done: mirror tests prove B3 remains the state-refresh path after B2 deletion.

- [ ] Remove active script usage of Java apply flags
  - Update `collect_remote_results.sh`.
  - Remove `-Dremote.exec.apply.trc10=false` from the Java startup command.
  - Search active scripts for `remote.exec.apply.` and remove any remaining Java apply flag usage.
  - Definition of done: active scripts no longer set `remote.exec.apply.*` JVM properties.

- [ ] Update Rust config comments for the new write ownership model
  - Update `rust-backend/config.toml` around the `rust_persist_enabled` comment block.
  - Update `rust-backend/crates/common/src/config.rs` around `RemoteExecutionConfig::rust_persist_enabled`.
  - Remove descriptions that name `RuntimeSpiImpl.apply*` as an active second write path.
  - State that canonical `RR` uses Rust as the writer and Java `postExecMirror` as read-side cache refresh.
  - State that `rust_persist_enabled=false` is legacy or unsupported for canonical `RR` execution, not a sanctioned compute-only profile.
  - Definition of done: config comments no longer instruct users to rely on Java apply in remote execution.

- [ ] Update authoritative close-loop planning docs
  - Update `planning/close_loop.bridge_debt.md` to mark B2 removed and remove it from the future removal queue.
  - Update `planning/close_loop.write_ownership.md` to remove `RR compute-only` as a sanctioned profile and state that `RuntimeSpiImpl.apply*` has been deleted.
  - Update `planning/close_loop.sidecar_parity.md` so sidecars are described as CSV/pre-state/mirror-validation data, not Java apply data.
  - Definition of done: current close-loop docs match the post-B2 ownership model.

- [ ] Leave historical planning files as historical artifacts
  - Do not bulk-edit old planning and todo files that document previous implementation phases unless they are referenced as current authority by the close-loop docs.
  - When an old file must be touched because it is referenced as current authority, add a short note that B2 Java apply has been removed and point to the updated close-loop ownership docs.
  - Definition of done: current docs are accurate without rewriting archived planning history.

- [ ] Run a source search for stale B2 runtime references
  - Search Java runtime source for `applyStateChangesToLocalDatabase`, `applyFreezeLedgerChanges`, `applyTrc10Changes`, `applyVoteChanges`, `applyWithdrawChanges`, `applyAssetIssuedChange`, `applyAssetTransferredChange`, `updateAccountState`, `deserializeAccountInfo`, and `remote.exec.apply.`.
  - Resolve matches in active source, tests, scripts, and current authoritative docs.
  - Definition of done: remaining matches are limited to historical planning artifacts explicitly left unchanged.

## Validation

- [ ] Compile Java runtime and tests
  - Run `./gradlew :framework:compileJava :framework:compileTestJava --dependency-verification=off`.
  - Definition of done: compilation succeeds.

- [ ] Run updated `RuntimeSpiImplTest`
  - Run `./gradlew :framework:test --tests "org.tron.common.runtime.RuntimeSpiImplTest" --dependency-verification=off`.
  - Definition of done: ownership and mirror-safety tests pass.

- [ ] Run mirror batch-get tests
  - Run `./gradlew :framework:test --tests "org.tron.core.storage.spi.RemoteStorageBatchGetTest" --dependency-verification=off`.
  - Definition of done: touched-key normalization and batch read behavior still pass.

- [ ] Run CSV/reporting tests that depend on sidecars
  - Run `./gradlew :framework:test --tests "org.tron.core.execution.reporting.ExecutionCsvRecordBuilderTest" --dependency-verification=off`.
  - Definition of done: CSV domain extraction still passes with remote sidecars retained.

- [ ] Run broader framework tests affected by execution SPI changes
  - Run `./gradlew :framework:test --tests "org.tron.core.db.ExecutionSpiIntegrationTest" --dependency-verification=off`.
  - Definition of done: execution SPI runtime selection behavior matches the new remote-only `RuntimeSpiImpl` gate.

- [ ] Verify stale apply symbols are gone from active code paths
  - Run a grep over active Java source, active scripts, Rust config comments, and current close-loop docs for `remote.exec.apply.` and deleted `apply*` method names.
  - Definition of done: active code and authoritative docs no longer reference Java apply as an available runtime path.

- [ ] Run canonical RR parity smoke for former B2 domains
  - Run the project’s RR parity smoke flow with `rust_persist_enabled=true` and `execution.mode=REMOTE`.
  - Cover account balance state changes, freeze/unfreeze/global resource totals, TRC-10 asset issue, TRC-10 transfer asset, vote witness, withdraw balance, VM storage, and contract metadata touched keys.
  - Definition of done: Rust persists the final state, Java mirrors from touched keys, and no parity run requires `remote.exec.apply.*` flags.

- [ ] Review the final diff for accidental B3 or reporting removal
  - Confirm `postExecMirror(...)`, pre-state snapshot capture, `RemoteExecutionSPI` sidecar conversion, and `ExecutionCsvRecordBuilder` sidecar usage remain present.
  - Definition of done: the diff removes B2 Java apply only and does not delete B3 mirror or reporting sidecars.
