# Close Loop Bridge Debt — B2 Removal Plan

Scope: `planning/close_loop.bridge_debt.md` B2 — delete the `RuntimeSpiImpl.apply*` family.

## Goal

Delete the transitional Java-side writer where `RuntimeSpiImpl` applies Rust-returned state/sidecar changes into Java chainbase.

This removes the old "Rust computes, Java writes" path and leaves canonical `RR` ownership as:

- Rust persists final state.
- Java only refreshes its read-side mirror with `postExecMirror(...)`.
- Java keeps remote sidecars for CSV/reporting and mirror validation, not for state application.

Do not delete B3 mirror, pre-state snapshot capture, or the `ExecutionSPI` result sidecars used by CSV reporting.

## Current entry point

Primary branch:

- `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:93-118`

Current behavior:

- `writeMode == PERSISTED`: Java skips apply and calls `postExecMirror(...)`.
- Otherwise: Java calls:
  - `applyStateChangesToLocalDatabase(...)`
  - `applyFreezeLedgerChanges(...)`
  - `applyTrc10Changes(...)`
  - `applyVoteChanges(...)`
  - `applyWithdrawChanges(...)`

## Target runtime behavior

Do not simply fail every `writeMode != PERSISTED` result. Rust may return `write_mode = 0` for failed/reverted executions with no committed effects.

Recommended rule:

1. `writeMode == ExecutionSPI.WriteMode.PERSISTED`
   - Call `postExecMirror(executionResult, context)`.
2. `writeMode != PERSISTED && executionResult.isSuccess()`
   - Fail fast: successful compute-only remote execution is no longer supported.
3. `writeMode != PERSISTED && has remote effects`
   - Fail fast: Java apply was removed, so effectful compute-only results cannot be accepted.
4. `writeMode != PERSISTED && !success && no effects`
   - Allow as a no-state failed/reverted result.

Conceptual replacement for the current branch:

```java
if (writeMode == ExecutionSPI.WriteMode.PERSISTED) {
  postExecMirror(executionResult, context);
} else if (executionResult.isSuccess() || hasRemoteStateEffects(executionResult)) {
  throw new IllegalStateException(
      "Remote execution returned COMPUTE_ONLY after Java-side apply was removed");
} else {
  logger.debug("No persisted state effects to mirror for unsuccessful remote execution");
}
```

`hasPersistedMirrorEffects(...)` currently exists at `RuntimeSpiImpl.java:1715-1724`. Keep the logic, but consider renaming it to `hasRemoteStateEffects(...)` because after B2 deletion it is no longer only a persisted-mirror check.

## Delete from `RuntimeSpiImpl`

Delete the five top-level B2 apply methods:

- `RuntimeSpiImpl.java:180` `applyStateChangesToLocalDatabase(...)`
- `RuntimeSpiImpl.java:213` `applyFreezeLedgerChanges(...)`
- `RuntimeSpiImpl.java:475` `applyTrc10Changes(...)`
- `RuntimeSpiImpl.java:524` `applyVoteChanges(...)`
- `RuntimeSpiImpl.java:612` `applyWithdrawChanges(...)`

Delete helper methods used only by that family:

- `RuntimeSpiImpl.java:270` `applyFreezeLedgerChange(...)`
- `RuntimeSpiImpl.java:320` `applyFreezeV1Change(...)`
- `RuntimeSpiImpl.java:359` `applyFreezeV2Change(...)`
- `RuntimeSpiImpl.java:436` `applyGlobalResourceChange(...)`
- `RuntimeSpiImpl.java:681` `applyAssetIssuedChange(...)`
- `RuntimeSpiImpl.java:800` `applyAssetTransferredChange(...)`
- `RuntimeSpiImpl.java:931` `applyStateChange(...)`
- `RuntimeSpiImpl.java:973` `updateAccountState(...)`
- `RuntimeSpiImpl.java:1104` `updateAccountStorage(...)`
- `RuntimeSpiImpl.java:1116` `bytesToLong(...)`
- `RuntimeSpiImpl.java:1130` `bytesToLongFromBalance(...)`
- `RuntimeSpiImpl.java:1146` nested `AccountInfo`
- `RuntimeSpiImpl.java:1198` `deserializeAccountInfo(...)`

`deserializeAccountInfo(...)` is not needed by CSV reporting. CSV parsing has independent logic in `DomainCanonicalizer.java:1612` and nearby methods.

## Keep in `RuntimeSpiImpl`

### B3 mirror support

Keep all mirror code:

- `RuntimeSpiImpl.java:45` `mirrorRemoteStorageSPI`
- `RuntimeSpiImpl.java:98-103` persisted-mode branch and `postExecMirror(...)` call
- `RuntimeSpiImpl.java:135-138` fatal handling for persisted mirror failure
- `RuntimeSpiImpl.java:1567-1593` mirror batch-get flags/constants
- `RuntimeSpiImpl.java:1608-1713` `postExecMirror(...)`
- `RuntimeSpiImpl.java:1715-1724` effect validation helper, possibly renamed
- `RuntimeSpiImpl.java:1750-1769` `normalizeTouchedKeys(...)`
- `RuntimeSpiImpl.java:1776-1835` `processBatchReads(...)`
- `RuntimeSpiImpl.java:1842-1867` `processPerKeyReads(...)`
- `RuntimeSpiImpl.java:1872-1880` `KeyOperation`
- `RuntimeSpiImpl.java:1882-1896` `getMirrorRemoteStorageSPI()`
- `RuntimeSpiImpl.java:1906-1975` `getStoreByDbName(...)`

These are B3, not B2.

### CSV / pre-state snapshot support

Keep:

- `RuntimeSpiImpl.java:90-91` `capturePreStateSnapshot(executionResult, context)` call
- `RuntimeSpiImpl.java:1353-1565` `capturePreStateSnapshot(...)`
- `RuntimeSpiImpl.java:1726-1739` `isV1UnfreezeContract(...)`

These support CSV/reporting parity and are not Java-side apply.

## Keep `ExecutionSPI` sidecars

Do not delete these result model types or conversions:

- `ExecutionSPI.StateChange`
- `ExecutionSPI.FreezeLedgerChange`
- `ExecutionSPI.GlobalResourceTotalsChange`
- `ExecutionSPI.Trc10Change`
- `ExecutionSPI.VoteChange`
- `ExecutionSPI.WithdrawChange`
- `ExecutionSPI.TouchedKey`

Reasons:

- `RemoteExecutionSPI.java:1756-2074` still converts Rust response fields into Java result objects.
- `ExecutionCsvRecordBuilder.java:149-238` still uses these fields for CSV domains and digests.
- `capturePreStateSnapshot(...)` uses sidecars to capture old state.
- The mirror safety check still uses sidecars to detect effectful persisted results without touched keys.

Update comments that say Java "should apply" these sidecars. They should describe them as legacy compute-only/reporting/mirror-validation sidecars.

## Runtime mode guard

`Manager.createRuntime()` currently creates `RuntimeSpiImpl` whenever `shouldUseExecutionSpi()` returns true:

- `framework/src/main/java/org/tron/core/db/Manager.java:2786-2800`

`shouldUseExecutionSpi()` does not explicitly require `execution.mode=REMOTE`:

- `framework/src/main/java/org/tron/core/db/Manager.java:2809-2825`

After B2 deletion, avoid accidentally routing embedded or shadow execution through the new compute-only rejection path.

Recommended change:

- Make `RuntimeSpiImpl` block execution a `REMOTE`-only runtime path.
- Keep `EMBEDDED` on `RuntimeImpl`.
- Treat `SHADOW` as legacy developer tooling and do not route it through this canonical runtime path unless a separate shadow-specific semantic is defined.

If `SHADOW` must remain supported here, explicitly bypass the compute-only rejection for shadow embedded results. Otherwise `ShadowExecutionSPI` returns the embedded result at `ShadowExecutionSPI.java:129-130`, whose `writeMode` defaults to `COMPUTE_ONLY`.

## Test plan changes

### Remove or rewrite apply-reflection tests

`framework/src/test/java/org/tron/common/runtime/RuntimeSpiImplTest.java` has reflection references to private apply methods:

- `RuntimeSpiImplTest.java:194-202`
- `RuntimeSpiImplTest.java:442-449`
- `RuntimeSpiImplTest.java:515-521`

Delete or rewrite these tests. They currently validate `applyTrc10Changes(...)` behavior, which no longer belongs in Java runtime after B2 removal.

Replacement coverage:

1. `RuntimeSpiImpl` rejects successful `COMPUTE_ONLY` remote results.
2. `RuntimeSpiImpl` rejects effectful non-persisted remote results.
3. `RuntimeSpiImpl` accepts failed/reverted non-persisted results only when there are no effects.
4. `RuntimeSpiImpl` keeps `PERSISTED + touched_keys` on the mirror path.
5. `PERSISTED + effects + empty touched_keys` still fails.
6. CSV/reporting tests still pass with sidecars present.

### Keep or supplement mirror tests

Keep touched-key normalization/batch mirror coverage, including `RemoteStorageBatchGetTest`.

If practical, add a runtime-level mirror test for touched-key refresh rather than private apply methods.

### Suggested commands

Use project-required dependency verification override:

```bash
./gradlew :framework:compileJava :framework:compileTestJava --dependency-verification=off
./gradlew :framework:test --tests "org.tron.common.runtime.RuntimeSpiImplTest" --dependency-verification=off
./gradlew :framework:test --tests "org.tron.core.storage.spi.RemoteStorageBatchGetTest" --dependency-verification=off
./gradlew :framework:test --tests "org.tron.core.execution.reporting.ExecutionCsvRecordBuilderTest" --dependency-verification=off
```

Then run an RR parity smoke covering former apply domains:

- Account balance state changes
- Freeze/unfreeze/global resource totals
- TRC-10 asset issue and transfer asset
- Vote witness
- Withdraw balance
- VM storage and contract metadata touched keys

## Script and doc cleanup

### Scripts

`collect_remote_results.sh:171` currently sets:

```bash
-Dremote.exec.trc10.enabled=true -Dremote.exec.apply.trc10=false
```

Remove `-Dremote.exec.apply.trc10=false`. After B2 deletion, `remote.exec.apply.*` flags should no longer exist or matter.

### Rust config comments

Update comments in:

- `rust-backend/config.toml:174-179`
- `rust-backend/crates/common/src/config.rs:176-205`

Current comments describe `RuntimeSpiImpl.apply*` as a second write path. After B2 deletion, they should say:

- Canonical RR writer is Rust.
- Java `postExecMirror` is read-side cache refresh.
- `rust_persist_enabled=false` is no longer a sanctioned RR compute-only profile; if retained at all, it is legacy/unsupported diagnostic behavior.

### Close-loop docs

Update current authoritative docs:

- `planning/close_loop.bridge_debt.md`
  - Mark B2 as removed.
  - Remove it from the future removal queue or mark it completed.
- `planning/close_loop.write_ownership.md`
  - Remove `RR compute-only` as a sanctioned profile.
  - State that `RuntimeSpiImpl.apply*` has been deleted.
- `planning/close_loop.sidecar_parity.md`
  - Change sidecar purpose from Java apply to CSV/pre-state/mirror validation.

Older planning/todo files can remain historical unless they are still treated as authoritative.

## Main risks

### 1. Incomplete Rust `touched_keys`

After B2 deletion, Java has no state-apply fallback. If Rust writes a DB key but omits it from `touched_keys`, Java's read-side mirror can go stale.

Defense:

- Keep effect-without-touched-keys validation.
- Run RR parity smoke for all former apply domains.

### 2. Successful `COMPUTE_ONLY` result

Previously Java applied it. After deletion, accepting it would silently lose state.

Defense:

- Fail fast on successful non-persisted remote results.

### 3. Failed/reverted non-persisted result

Some failed/reverted paths may return `write_mode = 0` with no effects. These should not be misclassified as B2 dependency.

Defense:

- Allow only `!success && no effects` non-persisted results.

### 4. `SHADOW` and `EMBEDDED` accidentally using `RuntimeSpiImpl`

Embedded-style results default to `COMPUTE_ONLY`.

Defense:

- Restrict `RuntimeSpiImpl` block execution to `REMOTE`, or define explicit shadow bypass semantics.

### 5. Accidentally deleting reporting sidecars

Sidecars are still needed for CSV/reporting and pre-state snapshot capture.

Defense:

- Delete only Java apply methods and apply-only helpers.
- Keep `RemoteExecutionSPI` conversions and `ExecutionProgramResult` fields.

## Recommended implementation order

1. Replace the `RuntimeSpiImpl.execute(...)` apply branch with persisted-mirror / non-persisted-fail-fast handling.
2. Delete the five top-level `apply*` methods.
3. Delete apply-only helpers and nested `AccountInfo`.
4. Clean imports.
5. Restrict `RuntimeSpiImpl` creation to the intended `REMOTE` path, or explicitly define `SHADOW` behavior.
6. Delete or rewrite `RuntimeSpiImplTest` apply-reflection tests.
7. Remove `remote.exec.apply.*` script/config/doc references.
8. Update `close_loop` planning docs to mark B2 removed.
9. Run compile, targeted tests, then RR parity smoke.
