review my current implementation, do they follow planning/emit_freeze_ledger_changes-7.planning.md and planning/emit_freeze_ledger_changes-7.todo.md correctly?

---

• Here’s a focused review of your last 7 commits against the plan and TODO. Net: you’ve implemented the core end-to-end path (proto → Rust emission → Java parse → Java apply) and tests, but there’s a critical
semantics issue for V2 application (absolute vs delta), and a couple of planned items are missing or intentionally deferred.

What matches the plan

- Proto changes
    - Added FreezeLedgerChange and GlobalResourceTotalsChange, and extended ExecutionResult with repeated fields: framework/src/main/proto/backend.proto:589, 612–613.
- Rust backend
    - Config flag emit_freeze_ledger_changes added; defaults false in code: rust-backend/crates/common/src/config.rs:213–228, 266–279; sample config in rust-backend/config.toml.
    - Emission implemented for:
        - FreezeBalance V1: rust-backend/crates/core/src/service.rs:1365–1424
        - UnfreezeBalance V1: rust-backend/crates/core/src/service.rs:1500–1545
        - FreezeBalanceV2: rust-backend/crates/core/src/service.rs:1698–1757
        - UnfreezeBalanceV2: rust-backend/crates/core/src/service.rs:1859–1921
    - Response builder maps internal freeze_changes -> protobuf: rust-backend/crates/core/src/service.rs:5161–5193.
    - TronExecutionResult carries freeze_changes: rust-backend/crates/execution/src/tron_evm.rs:200–206.
- Java parse + apply
    - Parsed in RemoteExecutionSPI: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java: parsed freeze/global lists into ExecutionSPI types; builds state changes with serialized
    AccountInfo.
    - ExecutionProgramResult extended with freezeChanges/globalResourceChanges: framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java:10–23.
    - Apply hook added:
        - Called after state changes: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:54–74 (execute), 93–141 (applyStateChanges), 146–207 (applyFreezeLedgerChanges).
        - V1: setFrozenForBandwidth/energy used (absolute): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:272–287.
        - Global totals: write to DynamicPropertiesStore (optional for now): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:324–348.
- Tests
    - Emission gated by flag, V2 variants, partial/full unfreeze covered in unit tests: rust-backend/crates/core/src/service.rs:2700–3360 and rust-backend/crates/core/src/tests.rs.

Gaps and deviations

- V2 semantics (critical)
    - Emitted amount for V2 is absolute (“total after operation”), but Java applies it as a delta by calling add*:
        - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:306 (BANDWIDTH), 311 (ENERGY), 316 (TRON_POWER).
    - AccountCapsule’s add* methods increment the existing value: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:534–566. This makes application non-idempotent and will double-count if a
    retry/re-apply happens.
    - Plan/TODO called for set/aggregate to the new value (absolute). The current implementation violates that.
- Dirty marking omitted (minor but recommended)
        - recordAccountDirty(owner) when updating accounts
        - recordDynamicKeyDirty(...) when applying global totals
    - Not present in RuntimeSpiImpl; see ResourceSyncContext usage elsewhere for reference (e.g., BandwidthProcessor).
- JVM toggle for rapid rollback (missing)
    - Plan/TODO: a property like -Dremote.exec.apply.freeze=false to bypass Java-side application for quick rollback.
    - No gate in RuntimeSpiImpl; applyFreezeLedgerChanges always runs when lists present.
- Optional global totals emission (deferred)
    - Rust always sends empty global_resource_changes; Java applies if present. This matches the TODO’s “deferred” adjustment, but note it won’t update totals unless Java recomputes elsewhere.
- Config/logging polish (minor)
    - Startup logging of AEXT mode/emit flag mentioned in TODO; not visible in the service init logs.

Concrete fixes to align fully

- Fix V2 absolute semantics (idempotent)
    - In RuntimeSpiImpl.applyFreezeV2Change, compute/set the absolute value instead of add:
        - Read current FreezeV2 entry for the resource (AccountCapsule.getFrozenV2List()).
        - If present, replace with a FreezeV2 at the target amount via updateFrozenV2List; if absent, add a new entry with the target amount.
        - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:300–321.
        - Reference setters: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:705–744 (updateFrozenV2List/addFrozenV2List) near the FreezeV2 helpers; and 534–566 shows add semantics you
        should avoid here.
- Mark dirty for resource processors
    - After writing AccountStore.put: call ResourceSyncContext.recordAccountDirty(ownerAddress).
    - After writing dynamic totals: call ResourceSyncContext.recordDynamicKeyDirty(...) for relevant keys.
    - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:230–239 (after put), 324–348 (after dynamic save).
- Add JVM toggle
    - Wrap applyFreezeLedgerChanges with a property check:
        - if (!Boolean.getBoolean("remote.exec.apply.freeze")) return;
    - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:146–207.
- Optional: add startup logs for the backend
    - Log accountinfo_aext_mode and emit_freeze_ledger_changes once at module init.

Verdict

- Core pipeline is in place and closely follows the plan/TODO for Phase 2. The proto, Rust emission with flag gating, Java parsing, and hook placement are correct, and you have tests to cover the emission
behavior.
- The main blocker is the V2 absolute-vs-delta mismatch in Java application. Fixing RuntimeSpiImpl to “set to absolute” for V2 will satisfy the idempotency requirement and align with the planned semantics.
- Secondary improvements are the dirty marks and a rollback toggle.

Want me to patch RuntimeSpiImpl to switch V2 to absolute semantics and add the dirty marks + property gate now?

---

