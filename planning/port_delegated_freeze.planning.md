• Goal
Achieve delegation parity so remote execution + remote storage yields the same tron power and witness ordering as embedded Java.

Current Behavior

- Java tron power includes own frozen (V1+V2, excluding TRON_POWER) + delegated-out (V1+V2) and excludes any acquired delegated resources. See:
    - chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:600
    - chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:626
    - chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:632
- Delegation ledger data lives in:
    - chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceCapsule.java:1
    - chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java:1
    - chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceAccountIndexCapsule.java:1
- Rust tron power sums only freeze-records (and previously included TRON_POWER), misses delegated-out:
    - rust-backend/crates/execution/src/storage_adapter/engine.rs:792

Phase 2 Plan (Delegation Parity)

- Data Model & Storage (Rust)
    - Add a “delegated-resources” database mirroring Java’s DelegatedResourceStore:
        - Key format: prefix + from(21) + to(21) [+ resource u8 if you split by resource], supporting both “lock” and “unlock” records (match V2 semantics).
        - Fields: resource (0=BANDWIDTH, 1=ENERGY), amount (i64 SUN), expire_time (ms).
        - Provide helpers: get_delegation(from,to,resource,lock), set_delegation, remove_delegation, unlock_expired(now).
    - Indexing for totals:
        - Outgoing totals: prefix scan by from address for BANDWIDTH/ENERGY, sum both V1 and V2 records.
        - Incoming totals: prefix scan by to address for BANDWIDTH/ENERGY (used for “acquired” totals; not counted in tron power).
- Execution Path (Contracts)
    - Implement DelegateResource/UnDelegateResource execution to mutate delegated-resources DB:
        - On delegate: create/update “lock” record with expire time (duration).
        - On unfreeze/expiry: move from lock → unlock; apply semantics in bulk at block execution time (unlock_expired(now)).
    - Emit state changes for both delegator and receiver accounts:
        - Delegator: update fields used by Java tron power:
            - delegatedFrozenBalanceForBandwidth (V1), AccountResource.delegatedFrozenBalanceForEnergy (V1)
            - delegatedFrozenV2BalanceForBandwidth (V2), AccountResource.delegatedFrozenV2BalanceForEnergy (V2)
        - Receiver: update “acquired delegated” fields for bandwidth/energy (affect resource usage, not tron power).
    - Wire through RemoteExecutionSPI for both contracts (mapped already) and ensure the Rust service returns Delegation changes.
- Tron Power Calculation (Rust)
    - Compute tron power with the same components as Java:
        - Own frozen V1 bandwidth + V1 energy (exclude TRON_POWER).
        - Own frozen V2 entries where type=BANDWIDTH/ENERGY (exclude TRON_POWER).
        - Delegated-out totals (V1+V2) for BANDWIDTH and ENERGY.
    - Implementation points to change:
        - rust-backend/crates/execution/src/storage_adapter/engine.rs:792 (extend compute_tron_power to include delegated-outs and ensure old-model excludes TRON_POWER, new-model can include TRON_POWER if
        aligned with Java’s getAllTronPower).
- State Sync to Java (Execution SPI)
    - Extend gRPC result to carry a dedicated delegation change list (similar to FreezeLedgerChange):
        - Fields: owner(from), receiver(to), resource, amount, expiration, v2_model, is_add/remove, is_unlock.
    - Apply on Java:
        - Extend RuntimeSpiImpl with applyDelegationChange that:
            - Updates delegator’s delegated fields (V1/V2) and receiver’s acquired delegated fields (V1/V2).
            - Writes DelegatedResourceStore entries to keep DB parity (createDbKeyV2 / lock/unlock).
            - Marks dirty via ResourceSyncContext (so ResourceSyncService flushes to remote storage).
        - File touchpoints:
            - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:120 (account changes)
            - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:260 (freeze changes pattern)
            - chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java:1 (persist entries)
            - chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceCapsule.java:1 (construct entries)
- Periodic Expiry Handling
    - Align with Java’s unlock logic:
        - chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java:16 (unLockExpireResource)
    - In Rust execution at block boundary, process lock entries whose expire < blockTime; update account fields and store entries; emit delegation changes for Java to reflect.
- Storage SPI Parity (Optional but Recommended)
    - Ensure Account protobufs in remote storage carry fields Java reads for tron power:
        - account.address, type, balance, create_time (existing)
        - V1 frozen list (for bandwidth), AccountResource.frozenBalanceForEnergy
        - Delegated-out V1 totals (delegatedFrozenBalanceForBandwidth, AccountResource.delegatedFrozenBalanceForEnergy)
        - V2 “frozenV2” list entries for BANDWIDTH/ENERGY
        - Delegated-out V2 totals (delegatedFrozenV2BalanceForBandwidth, AccountResource.delegatedFrozenV2BalanceForEnergy)
    - Update engine-side serializer used by remote storage:
        - rust-backend/crates/execution/src/storage_adapter/engine.rs:120 (serialize_account)
        - Build a minimal Protocol.Account that includes the above fields (others default).
    - Rationale: Even if Java receives state changes, Java’s own stores and any reads through AccountStore (remote) must see consistent bytes.
- Resource Accounting Parity
    - Receiver’s acquired delegated fields affect bandwidth/energy usage paths in Java:
        - AccountCapsule getters e.g. getAcquiredDelegatedFrozenBalanceForEnergy (stored under AccountResource)
    - Ensure remote execution:
        - Updates both delegator and receiver AEXT/resource tails appropriately (for intra-block operations).
        - Emits global totals (TOTAL_NET_WEIGHT, TOTAL_ENERGY_WEIGHT, LIMIT) as needed (RuntimeSpiImpl already applies them; see framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:300).
- Dynamic Properties & Flags
    - Read resource model toggle from DynamicPropertiesStore to branch old/new model behavior:
        - consensusDelegate.getDynamicPropertiesStore().supportAllowNewResourceModel()
    - Honor committee toggles consistently in Rust execution and in tron power logic.
- Testing Strategy
    - Unit tests (Rust):
        - Freeze V1/V2; delegate bandwidth/energy; compute tron power; expect:
            - Old model: sum(own V1+V2 BW/EN) + delegated-out V1+V2; exclude TRON_POWER.
            - New model: same, potentially include TRON_POWER if Java does via getAllTronPower.
        - UnDelegate and expiry scenarios: totals decrease correctly and are reflected in account fields.
    - Integration tests:
        - Seed a small chain slice: perform Freeze → Delegate → VoteWitness; verify Java + Rust produce identical vote accept/reject decisions and witness ordering after maintenance.
        - Toggle model flag mid-run and verify consistency across boundaries.
    - Performance: cache per-account delegated-out totals to avoid full scans; invalidate on mutations.
- Migration/Bootstrap
    - On startup, read existing DelegatedResourceStore and AccountStore from remote storage to reconstruct delegated totals cache in Rust (if using a cache).
    - Optional: one-time reconciliation job to verify Java AccountCapsule delegated totals equal the sum of delegated resource entries (warn on drift).

Optional Enrichment: Remote Account Serialization

- Extend remote storage Account serialization to write a fuller Protocol.Account (fields that Java reads for tron power and resource usage). This improves resilience if Java restarts and rebuilds state purely
from remote storage.
- Touchpoints:
    - rust-backend/crates/execution/src/storage_adapter/engine.rs:120 (serialize_account)
    - Include frozen lists, delegated totals (V1+V2) and AccountResource submessage.
- Note: Execution state changes already apply to Java DB via RuntimeSpiImpl; this enrichment reduces reliance on state-change-only flows and helps cold reads.

Rollout Order

1. Implement delegated-resources DB + compute_tron_power including delegated-out.
2. Implement DelegateResource/UnDelegateResource execution + delegation state changes.
3. Apply delegation changes on Java (RuntimeSpiImpl) and flush via ResourceSyncService.
4. Add expiry handling and tests.
5. Optional: enrich remote Account protobuf for storage SPI.
6. Verify parity on the same chain slice around maintenance boundary (the previously failing point).

If you want, I can map out the concrete data structures and exact protobuf fields for Account and DelegatedResource entries, plus the minimal ExecutionSPI extensions for DelegationChange, before we begin
coding.

---

