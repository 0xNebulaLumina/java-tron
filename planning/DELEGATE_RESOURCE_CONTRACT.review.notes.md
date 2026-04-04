# DELEGATE_RESOURCE_CONTRACT Review Notes

**Review Date:** 2026-04-04
**Branch:** `Theseus_compare_DELEGATE_RESOURCE_CONTRACT.remaining`
**Commit:** `b52014a21a`
**Reviewer:** Claude (automated)

---

## 1. Baseline Inventory

### 1.1 Original Unresolved Statements

Source: `planning/review_again/DELEGATE_RESOURCE_CONTRACT.todo.md`

The original TODO contains three open items (lines 91-106):

1. **Decay tests (currently ignored - require investigation):**
   - `test_delegate_resource_usage_decay_increases_available` (ignored)
   - `test_delegate_resource_expired_usage_fully_resets` (ignored)
   - Note: "These tests fail when net_usage > 0 with old timestamps. Core validation works."

2. **Validate end-to-end:**
   - Run existing conformance tests that cover resource delegation (fixtures under `ResourceDelegationFixtureGeneratorTest.java`)
   - If remote execution is used, run a remote-vs-embedded parity diff on a delegation-heavy fixture set

### 1.2 All Files Related to Delegation

#### Contract Definitions
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java` (line 38)
- `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java` (line 31)
- `actuator/src/main/java/org/tron/core/vm/nativecontract/DelegateResourceProcessor.java` (line 32)
- `actuator/src/main/java/org/tron/core/vm/nativecontract/UnDelegateResourceProcessor.java`
- `actuator/src/main/java/org/tron/core/vm/nativecontract/param/DelegateResourceParam.java`
- `actuator/src/main/java/org/tron/core/vm/nativecontract/param/UnDelegateResourceParam.java`

#### Stores / Indexes
- `chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java` (line 15) — DB: "DelegatedResource"
- `chainbase/src/main/java/org/tron/core/store/DelegatedResourceAccountIndexStore.java` (line 19) — DB: "DelegatedResourceAccountIndex"
- `chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceCapsule.java` (line 16)
- `chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceAccountIndexCapsule.java` (line 16)

#### Validation Logic
- `actuator/src/main/java/org/tron/core/vm/utils/FreezeV2Util.java` — `getV2NetUsage`, `getV2EnergyUsage`
- `chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java` — `updateUsageForDelegated`
- `chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java` — `updateUsage`

#### Query Paths
- `framework/src/main/java/org/tron/core/services/http/DelegateResourceServlet.java`
- `framework/src/main/java/org/tron/core/services/http/UnDelegateResourceServlet.java`
- `framework/src/main/java/org/tron/core/services/http/GetDelegatedResourceServlet.java`
- `framework/src/main/java/org/tron/core/services/http/GetDelegatedResourceV2Servlet.java`
- `framework/src/main/java/org/tron/core/services/http/GetDelegatedResourceAccountIndexServlet.java`
- `framework/src/main/java/org/tron/core/services/http/GetDelegatedResourceAccountIndexV2Servlet.java`
- `framework/src/main/java/org/tron/core/Wallet.java` — `getDelegatedResource`, `getDelegatedResourceV2`, `getDelegatedResourceAccountIndex`, `getDelegatedResourceAccountIndexV2`, `getCanDelegatedMaxSize`

#### Rust Backend
- `rust-backend/crates/core/src/service/mod.rs` — `execute_delegate_resource_contract()` (line ~7074), `execute_undelegate_resource_contract()`
- `rust-backend/crates/execution/src/storage_adapter/engine.rs` — V1/V2 delegation storage operations (lines 4593-5269)
- `rust-backend/crates/execution/src/storage_adapter/key_helpers.rs` — Key format helpers (lines 59-130)
- `rust-backend/crates/execution/src/storage_adapter/db_names.rs` — Database name constants
- `rust-backend/crates/common/src/config.rs` — `delegate_resource_enabled`, `undelegate_resource_enabled` flags

#### Existing Tests
- `framework/src/test/java/org/tron/core/actuator/DelegateResourceActuatorTest.java` — 17 @Test methods
- `framework/src/test/java/org/tron/core/actuator/UnDelegateResourceActuatorTest.java` — 12 @Test methods
- `framework/src/test/java/org/tron/core/conformance/ResourceDelegationFixtureGeneratorTest.java` — 59 @Test methods
- `framework/src/test/java/org/tron/core/db/DelegatedResourceStoreTest.java` — 3 @Test methods
- `framework/src/test/java/org/tron/core/db/DelegatedResourceAccountIndexStoreTest.java` — 8 @Test methods
- `framework/src/test/java/org/tron/core/WalletTest.java` — 8 delegation-related @Test methods
- `framework/src/test/java/org/tron/core/services/RpcApiServicesTest.java` — 5 delegation-related @Test methods
- `framework/src/test/java/org/tron/common/runtime/vm/FreezeV2Test.java` — `testDelegateResourceOperations()`
- `rust-backend/crates/core/src/service/tests/contracts/delegate_resource.rs` — 10 Rust tests
- `rust-backend/crates/core/src/service/tests/contracts/undelegate_resource.rs` — Rust undelegate tests

### 1.3 Dual-Mode / Storage-Parity Test Harnesses

- `framework/src/test/java/org/tron/core/storage/spi/DualStorageModeIntegrationTest.java` — 4 tests, **no delegation scenarios**
- `framework/src/test/java/org/tron/core/execution/spi/ShadowExecutionSPITest.java` — 10 tests, **no delegation scenarios**
- `framework/src/test/java/org/tron/core/storage/spi/StorageSpiFactoryTest.java` — storage factory tests
- `framework/src/test/java/org/tron/core/execution/reporting/ExecutionCsvRecordTest.java` — CSV record tests

**Finding:** No dual-mode test harness currently exercises delegation-specific scenarios.

### 1.4 Rust Backend Modules Affecting Delegation

- `rust-backend/crates/core/src/service/mod.rs` — Primary execution logic
- `rust-backend/crates/execution/src/storage_adapter/engine.rs` — Storage read/write for DelegatedResource and DelegatedResourceAccountIndex databases
- `rust-backend/crates/execution/src/storage_adapter/key_helpers.rs` — Key format (V2 prefix 0x01 unlock, 0x02 lock; index prefix 0x03 from, 0x04 to)
- `rust-backend/crates/core/src/conformance/runner.rs` — Conformance test runner (has `#[ignore]` on `test_run_real_fixtures` requiring env var)

---

## 2. Ignored Decay Tests

### 2.1 Search Results

Searched all Java and Rust test files for `@Ignore`, `@Disabled`, `#[ignore]`, and commented-out `@Test` annotations related to delegation, decay, recovery, window, bandwidth, or energy.

**Java tests:** Zero ignored delegation/decay tests found. The `@Ignore` annotations found in the codebase are all unrelated:
- `OperationsTest.java:784` — `testComplexOperations()` (VM operations)
- `TimeBenchmarkTest.java:24` — Entire class (benchmark)
- `StorageTest.java:189` — `testParentChild()` (VM storage)
- `LibrustzcashTest.java:275` — `calBenchmarkSpendConcurrent()` (zksnark)
- `SendCoinShieldTest.java:643` — `checkZksnark()` (zksnark)
- `ShieldedTRC20BuilderTest.java:83,121,252` — Multiple shielded TRC20 tests

**Rust tests:** The two decay tests referenced in the original TODO (`test_delegate_resource_usage_decay_increases_available` and `test_delegate_resource_expired_usage_fully_resets`) are located at:
- `rust-backend/crates/core/src/service/tests/contracts/delegate_resource.rs:1244`
- `rust-backend/crates/core/src/service/tests/contracts/delegate_resource.rs:1406`

**Critical finding:** These tests are **no longer ignored**. The `#[ignore]` annotation has been removed from both tests. They are currently active `#[test]` functions.

### 2.2 Ignored Decay Test Inventory Table

| Test Class / File | Test Method | Annotation | Ignore Reason | Feature Protected | Blocker Type | Status |
|---|---|---|---|---|---|---|
| `delegate_resource.rs` | `test_delegate_resource_usage_decay_increases_available` (line 1244) | `#[test]` (was `#[ignore]`) | Originally: "fail when net_usage > 0 with old timestamps" | Decay of bandwidth usage over time increases available delegation balance | Was: stale test / timing assumption | **No longer ignored** |
| `delegate_resource.rs` | `test_delegate_resource_expired_usage_fully_resets` (line 1406) | `#[test]` (was `#[ignore]`) | Originally: same as above | Fully expired usage resets to zero, making full frozen balance available | Was: stale test / timing assumption | **No longer ignored** |

### 2.3 Verdict on "Two decay tests are still ignored"

**Status: FALSE**

The two decay tests (`test_delegate_resource_usage_decay_increases_available` and `test_delegate_resource_expired_usage_fully_resets`) referenced in `planning/review_again/DELEGATE_RESOURCE_CONTRACT.todo.md` (lines 91-94) are **no longer ignored**. The `#[ignore]` annotations have been removed at some point after the TODO was written. Both tests are now regular `#[test]` functions.

The only `#[ignore]` in the Rust delegation test area is `test_run_real_fixtures` in `conformance/runner.rs:1402`, which is intentionally ignored (requires `CONFORMANCE_FIXTURES_DIR` env var) and is unrelated to decay tests.

---

## 3. Delegation Conformance Matrix

### 3.1 Scenario Coverage

| # | Scenario | Java Unit (Actuator) | Java Integration (Conformance Fixture) | Rust Unit | Dual-Mode | Missing |
|---|---|---|---|---|---|---|
| 1 | Delegate bandwidth to another account | `testDelegateResourceForBandwidth` | `generateDelegateResource_happyPath` | `test_delegate_resource_bandwidth_succeeds_when_usage_allows_delegation` | None | Dual-mode parity |
| 2 | Delegate energy to another account | `testDelegateResourceForCpu` | `generateDelegateResource_energy` | `test_delegate_resource_energy_succeeds_when_usage_allows_delegation` | None | Dual-mode parity |
| 3 | Undelegate bandwidth | `testUnDelegateForBandwidth` | `generateUnDelegateResource_happyPath` | Rust undelegate tests | None | Dual-mode parity |
| 4 | Undelegate energy | `testUnDelegatedForCpu` | (implicit in fixture gen) | Rust undelegate tests | None | Dual-mode parity |
| 5 | Partial undelegation | `testPartialUnDelegateForBandwidth`, `testPartialUnDelegatedForCpu` | (not explicit) | (not explicit) | None | Fixture + dual-mode |
| 6 | Full undelegation | `testUnDelegateForBandwidth` | `generateUnDelegateResource_fullUndelegateDeletesStoreAndIndex` | (not explicit) | None | Dual-mode parity |
| 7 | Invalid receiver / self-delegation / missing account | `testDelegateResourceToSelf`, `testDelegateResourceWithContractAddress` | `generateDelegateResource_toSelf`, `_ownerAccountNotExist`, `_receiverAccountNotExist`, `_receiverIsContractAccount` | `test_delegate_resource_fails_self_delegation` | None | N/A (validation only) |
| 8 | Insufficient / invalid amount boundaries | `testDelegateResourceWithNoFreeze`, `testDelegateBandwidthWithUsage`, `testDelegateCpuWithUsage` | `generateDelegateResource_insufficientFrozen`, `_delegateBalanceLessThan1TRX`, `_delegateBalanceExact1TRX` | `test_delegate_resource_bandwidth_fails_when_usage_exceeds_available`, `test_delegate_resource_fails_below_minimum` | None | N/A (validation only) |
| 9 | Lock/unlock interaction | `testLockedDelegateResourceForBandwidth`, `testMaxDelegateLockPeriod*` (5 tests), `testLockedUnDelegateForBandwidth`, `testLockedAndUnlockUnDelegateForBandwidth*` (2 tests) | `_withLock`, `_lockPeriodNegative`, `_lockPeriodExceedsMax`, `_lockPeriodLessThanRemainingPreviousLock`, `_lockPeriodZeroDefaults`, `_onlyLockedDelegationNotExpired`, `_lockedExpireTimeEqualsNow` | `test_delegate_resource_with_lock_fails_same_as_without_lock`, `test_delegate_resource_with_lock_succeeds_when_available` | None | Dual-mode parity |
| 10 | Decay/recovery after delegation | (implicit via `testDelegateBandwidthWithUsage`, `testDelegateCpuWithUsage`) | (not explicit) | `test_delegate_resource_usage_decay_increases_available`, `test_delegate_resource_expired_usage_fully_resets` | None | End-to-end decay + dual-mode |

### 3.2 Coverage by Layer

| Scenario | Validation Path | Execution Path | Receipt/Result | Account State | DelegatedResource Store | Account Index | Query Observable | Embedded Mode | Remote Mode |
|---|---|---|---|---|---|---|---|---|---|
| Delegate BW | Yes | Yes | Yes (SUCESS code) | Yes (frozenV2, delegatedV2) | Yes | Yes | Yes (WalletTest) | Yes | No explicit test |
| Delegate Energy | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | No explicit test |
| Undelegate BW | Yes | Yes | Yes | Yes (usage transfer) | Yes | Yes | Partial | Yes | No explicit test |
| Undelegate Energy | Yes | Yes | Yes | Yes | Yes | Yes | Partial | Yes | No explicit test |
| Partial Undelegate | Yes | Yes | Yes | Yes | Yes (partial entry) | Yes (retained) | No | Yes | No |
| Full Undelegate | Yes | Yes | Yes | Yes | Yes (deletion) | Yes (cleared) | No | Yes | No |
| Lock/Unlock | Yes | Yes | Yes | Yes | Yes | Yes | No | Yes | No |
| Decay/Recovery | Partial | Partial | No | Partial | No | No | No | Partial | No |

### 3.3 Gap Classification

| Scenario | Status | Gap Description |
|---|---|---|
| Delegate BW happy path | **Partially covered** | Java unit + fixture gen + Rust unit all cover this. Missing: dual-mode parity test |
| Delegate Energy happy path | **Partially covered** | Same as above |
| Undelegate BW | **Partially covered** | Java unit + fixture gen. Missing: dual-mode parity |
| Undelegate Energy | **Partially covered** | Java unit. Missing: explicit fixture + dual-mode parity |
| Partial undelegation | **Unit-only** | Java unit tests exist. No fixture gen, no dual-mode |
| Full undelegation | **Partially covered** | Java unit + fixture gen. No dual-mode |
| Invalid/boundary | **Covered end-to-end** | Both Java and Rust validate correctly |
| Lock/unlock | **Partially covered** | Good Java unit + fixture coverage. No dual-mode |
| Decay/recovery | **Unit-only** | Rust unit tests only. No Java-side decay test for delegation. No dual-mode |
| Deleted/recreated receiver | **Partially covered** | Java unit tests for both BW and Energy. No dual-mode |

### 3.4 Conformance Verdict

**"End-to-end conformance for resource delegation has not been completed"** — **CONFIRMED STILL TRUE**

Evidence:
1. No dual-mode (embedded-vs-remote) parity test exists for any delegation scenario
2. Decay/recovery interaction coverage is limited to Rust unit tests
3. Query-observable post-state is not verified end-to-end for most delegation flows
4. The `ResourceDelegationFixtureGeneratorTest` generates fixtures but there is no evidence these fixtures have been run through the Rust conformance runner for delegation scenarios

---

## 4. Remote-vs-Embedded Parity Review

### 4.1 Existing Dual-Mode Infrastructure

| Test File | Delegation Coverage |
|---|---|
| `DualStorageModeIntegrationTest.java` | None — tests storage mode switching only |
| `ShadowExecutionSPITest.java` | None — tests shadow execution framework, not delegation |
| `StorageSpiFactoryTest.java` | None — tests factory configuration |
| `ExecutionCsvRecordTest.java` | None — tests CSV record format |

### 4.2 Parity Diff Status

| Scenario | Embedded Result | Remote Result | Parity Verified |
|---|---|---|---|
| Delegate BW | Available (Java actuator tests) | Available (Rust unit tests) | **NOT compared** |
| Delegate Energy | Available | Available | **NOT compared** |
| Undelegate BW | Available | Available | **NOT compared** |
| Undelegate Energy | Available | Available | **NOT compared** |
| Lock/Unlock | Available | Available | **NOT compared** |
| Decay | Available (partial) | Available (Rust unit) | **NOT compared** |

### 4.3 Parity Verdict

**"Remote-vs-embedded parity diff has not been completed"** — **CONFIRMED STILL TRUE**

Evidence:
1. No dual-mode test harness exercises delegation scenarios
2. No parity diff artifacts exist for delegation flows
3. The `ResourceDelegationFixtureGeneratorTest` can produce fixtures, but no evidence exists that these have been consumed by the Rust conformance runner (`test_run_real_fixtures` in `conformance/runner.rs` requires manual setup via env var)
4. Individual test suites exist for both Java (embedded) and Rust (remote) paths, but they use different test fixtures and cannot be directly compared

---

## 5. Original TODO Verdicts

| # | Claim | Verdict | Evidence | Remaining Action |
|---|---|---|---|---|
| 1 | "Two decay tests are still ignored" | **FALSE** | Both `test_delegate_resource_usage_decay_increases_available` (line 1244) and `test_delegate_resource_expired_usage_fully_resets` (line 1406) in `delegate_resource.rs` no longer have `#[ignore]` annotations. They are active tests. | Update `planning/review_again/DELEGATE_RESOURCE_CONTRACT.todo.md` lines 91-94 to reflect that these tests are now active. Verify they pass by running `cargo test` in rust-backend. |
| 2 | "End-to-end conformance for resource delegation has not been completed" | **CONFIRMED STILL OPEN** | No dual-mode test exercises delegation. Fixture generator exists but fixtures have not been verified through the Rust conformance runner. Coverage matrix shows gaps in decay/recovery, query-observable state, and partial undelegation scenarios. | (1) Run `ResourceDelegationFixtureGeneratorTest` to generate fixtures. (2) Run Rust conformance runner against those fixtures. (3) Add delegation scenarios to dual-mode test harness. |
| 3 | "Remote-vs-embedded parity diff for delegation flows is not complete" | **CONFIRMED STILL OPEN** | No parity diff artifacts exist. No dual-mode test harness exercises delegation scenarios. | (1) Create deterministic shared test fixtures. (2) Run same scenarios in embedded and remote mode. (3) Compare outputs field-by-field. |

---

## 6. Recommendations

### 6.1 Minimum Tests to Add for Conformance

1. **Dual-mode delegation test**: Add delegation scenarios to `ShadowExecutionSPITest` or create a new `DelegationParityTest` that runs delegate BW, delegate Energy, undelegate BW, undelegate Energy in both modes and compares results.
2. **Decay integration test**: Add a Java-side test that exercises the "available FreezeV2 after usage" path with non-zero decayed usage, matching the Rust decay tests.
3. **Fixture validation**: Run `ResourceDelegationFixtureGeneratorTest` fixtures through `test_run_real_fixtures` in the Rust conformance runner.

### 6.2 Minimum Dual-Mode Scenarios to Automate

1. Delegate bandwidth (happy path) — compare account state, delegated resource store, index
2. Delegate energy (happy path) — same
3. Full undelegate — verify store deletion and index cleanup in both modes
4. Delegate with lock — verify lock expiration semantics match

### 6.3 Decay Test Determinism

The decay tests (`test_delegate_resource_usage_decay_increases_available`, `test_delegate_resource_expired_usage_fully_resets`) use `latest_consume_time = 0` with `current_slot = 1000000`, which is deterministic (no real-time dependency). These tests should be stable. If they were previously failing, the fix was likely in the decay calculation logic itself, not in test timing.

### 6.4 TODO Split Recommendation

The original TODO should be split into:
1. **Decay tests** — Can be closed (tests are now active and un-ignored)
2. **End-to-end conformance** — Separate workstream requiring fixture runner integration
3. **Remote-vs-embedded parity** — Separate workstream requiring dual-mode test infrastructure enhancement
