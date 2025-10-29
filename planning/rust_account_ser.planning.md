Ensure the remote storage’s AccountCapsule serialization/deserialization includes the resource usage fields (not just balance/nonce/codeHash)

---

• Here’s a precise, end‑to‑end plan to include AccountCapsule resource usage in the remote path, with backward compatibility and clear ownership across Java and Rust.

What Exists Today

- Account change serialization (embedded journal): framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:260
    - Emits balance[32] + nonce[8] + codeHash[32] + codeLen[4] + code, no resource fields.
- Account change serialization from remote proto: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:476
    - Same minimal format; builds bytes from backend AccountInfo proto.
- Account change deserialization and application to local DB (remote mode): framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:358
    - Parses the minimal format; only updates balance; logs code/codeHash; doesn’t touch resource fields.
- Backend proto (shared by Java + Rust): framework/src/main/proto/backend.proto:590
    - AccountInfo = address, balance, nonce, code_hash, code; no resource usage.
- AccountCapsule resource usage fields present and used across the system:
    - Net/energy usage + times + windows: chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:1056, :1186, :1159, :342, :350, :1176, :1385, :1425

Goal

- Ensure the remote storage/execution round‑trip for AccountCapsule includes resource usage attributes (at minimum: netUsage, freeNetUsage, energyUsage, latestConsumeTime, latestConsumeFreeTime,
latestConsumeTimeForEnergy; optionally window size/optimized flags), not just balance/nonce/codeHash/code.

Design Choices

- Serialization backward compatibility:
    - Keep current base layout intact.
    - Append an optional “AccountInfo Extension” after code bytes, identified by a magic + version + length, so older readers ignore it and newer readers parse it.
- Wire format extension (for both embedded journal and remote):
    - Base (unchanged): balance[32] + nonce[8] + codeHash[32] + codeLen[4] + code[codeLen]
    - Optional tail:
        - Magic: ASCII “AEXT” (4 bytes)
        - Version: 1 (u16 big‑endian)
        - Length: N (u16 big‑endian)
        - Payload v1 (fixed layout; all big‑endian unless otherwise stated)
            - netUsage i64 (8)
            - freeNetUsage i64 (8)
            - energyUsage i64 (8)
            - latestConsumeTime i64 (8)
            - latestConsumeFreeTime i64 (8)
            - latestConsumeTimeForEnergy i64 (8)
            - netWindowSize i64 (8)
            - energyWindowSize i64 (8)
            - netWindowOptimized bool (1)
            - energyWindowOptimized bool (1)
            - reserved/pad (2) to keep length even (optional)
- Why: Byte‑tail is simple, versioned, and requires minimal Rust changes to start (Java can emit/consume now; Rust can add population later). If/when we want Rust to explicitly carry structured resource usage
via gRPC, we can add fields to AccountInfo as optional and have Java bridge populate the AEXT tail from those when present.

Planned Changes (Java)

- Deserialization update (must-have):
    - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:358
    - Extend deserializeAccountInfo(byte[] data):
        - After parsing base fields, if offset < data.length and data[offset..offset+4] == "AEXT", parse the v1 payload.
        - Populate AccountCapsule with:
            - setNetUsage(v)
            - setFreeNetUsage(v)
            - setEnergyUsage(v)
            - setLatestConsumeTime(v)
            - setLatestConsumeFreeTime(v)
            - setLatestConsumeTimeForEnergy(v)
            - For window settings:
                - setNewWindowSize(BANDWIDTH, netWindowSize), setWindowOptimized(BANDWIDTH, netWindowOptimized)
                - setNewWindowSize(ENERGY, energyWindowSize), setWindowOptimized(ENERGY, energyWindowOptimized)
        - If AEXT missing, leave resource fields unchanged (backward compatible).
- Embedded journal serialization (ensures parity and round‑trip testability):
    - File: framework/src/main/java/org/tron/core/execution/reporting/StateChangeJournal.java:260
    - Update serializeAccountInfo(AccountCapsule):
        - Produce current base layout.
        - Append AEXT v1 payload populated from the AccountCapsule getters:
            - getNetUsage(), getFreeNetUsage(), getEnergyUsage()
            - getLatestConsumeTime(), getLatestConsumeFreeTime(), getAccountResource().getLatestConsumeTimeForEnergy()
            - getWindowSizeV2(BANDWIDTH|ENERGY) or raw if you prefer exact stored values
            - getWindowOptimized(BANDWIDTH|ENERGY)
        - Gate emission with a system property if desired (e.g., -Dremote.exec.accountinfo.resources.enabled=true); default on in REMOTE mode.
- Remote response bridge serialization (best effort, supports future Rust additions):
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:476
    - Update serializeAccountInfo(BackendOuterClass.AccountInfo):
        - Always write the existing base layout.
        - Append AEXT v1 tail only if we can source values:
            - If proto gains resource fields (see Rust/proto plan), use those.
            - Otherwise, omit tail to avoid incorrect values; resource updates continue to be handled by Java resource processors.
    - Note: Do not add DB dependencies here; keep it pure conversion from proto. The tail is optional.
- No changes needed in StorageSPI; account bytes stored via AccountCapsule.getData() already include resource usage in the protobuf.

Planned Changes (Proto + Rust)

- Extend backend proto (optional but recommended for remote-first completeness):
    - File: framework/src/main/proto/backend.proto:590
    - Add optional fields to message AccountInfo:
        - int64 net_usage = 6;
        - int64 free_net_usage = 7;
        - int64 energy_usage = 8;
        - int64 latest_consume_time = 9;
        - int64 latest_consume_free_time = 10;
        - int64 latest_consume_time_for_energy = 11;
        - int64 net_window_size = 12;
        - bool net_window_optimized = 13;
        - int64 energy_window_size = 14;
        - bool energy_window_optimized = 15;
    - Rebuild Java and Rust stubs (Rust uses this proto via build.rs).
- Populate these fields in Rust (incremental, safe default is to leave unset):
    - File: rust-backend/crates/core/src/service.rs in convert_execution_result_to_protobuf when building AccountChange and AccountInfo.
    - Initial approach:
        - If the backend can’t compute accurate per‑account usage yet, leave fields unset (Java will not append AEXT). This keeps compatibility now and enables future enrichment.
        - Later enhancement: feed per-tx owner account resource usage using known ExecutionResult.resource_usage and tx metadata, or read persisted account bytes from storage engine and decode TRON
        protocol.Account resource fields to populate absolute values.

Backward Compatibility

- Old backends and old CSV/journal data remain valid:
    - Deserializer ignores tail if missing.
    - Serializer can be gated by a property; default behavior only extends in remote mode or when explicitly enabled.
- No changes in StorageSPI; remote DB continues to store full TRON protocol.Account bytes, unaffected by this plan.

Testing Plan

- Unit tests
    - Add round‑trip test for journal serialization/deserialization:
        - Build an AccountCapsule with non‑default resource usage.
        - serializeAccountInfo then deserializeAccountInfo; assert all resource fields match.
    - Add deserialization tests for:
        - Base‑only payload → resource fields unchanged.
        - Corrupt/short AEXT → ignored safely, logs warn.
- Integration tests
    - Extend DualStorageModeIntegrationTest or add a new test that:
        - Executes a tx in remote mode with StateChange journaling enabled.
        - Verifies local AccountCapsule resource usage matches expected values after RuntimeSpiImpl.applyStateChangesToLocalDatabase() if AEXT present; otherwise resource usage unaffected by account change
        bytes (still updated by existing processors).
- Proto/Rust compilation checks
    - Build Java with updated proto.
    - Build Rust; ensure tonic stubs regenerate and compile.
- Optional: CSV/state digest
    - If you compute digests on state change payloads, update any expectations to account for the appended bytes; otherwise tests should stay green.

Rollout and Config

- Flags
    - remote.exec.accountinfo.resources.enabled (default true in REMOTE) to append AEXT tail.
- Deployment
    - Commit Java changes first (safe without Rust changes; AEXT only added in journal).
    - Update proto and Rust next; Java bridges start appending AEXT for remote once backend supplies fields.
- Docs
    - Document the AEXT tail in a short spec under docs (structure, versioning, fields).
    - Note that StorageSPI continues to store full protocol.Account bytes with resource usage; AEXT is for account change synchronization only.

Risks and Mitigations

- Incomplete remote data: If Rust doesn’t populate resource fields, Java won’t append AEXT in the remote bridge; resource usage continues to be updated by existing Java processors.
- Version drift: AEXT is versioned and optional; older nodes ignore tail.
- Misapplied absolute vs delta: Commit to absolute values in AEXT (not deltas) to keep semantics simple; the Java deserializer sets fields directly.
