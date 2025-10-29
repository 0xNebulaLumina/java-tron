Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20251027-040703-4abab9f8-embedded-embedded.csv
+ output-directory/execution-csv/20251027-121232-7db19f84-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 2.embedded-java.log
(remote execution + remote storage) java log: remote-java.df135ac.log
(remote execution + remote storage) rust log: remote-rust.df135ac.log


---

• Here’s the first divergence and why it happens.

- First mismatch: block 2153, tx 8ab04add0d317bba53d2f3abe3ed79dbd675234903fcb3764c2adf91e7f6856e
- Contract: VoteWitnessContract
- Outcome: both success, energy_used 0, but state_changes_json and state_digest_sha256 differ

What differs

- CSV row index: 1043 in both files
- state_digest_sha256:
    - embedded: 80fac92e0944c8f4a74cd29b6fae49042b9594a33f88c5bcf7d3684b034a5c93
    - remote:   12eb235be4b6912556f7dac1cfa1bcff55b6a9f4068c76f7e7505be4c98d747f
- state_changes_json: same address (account 41e552f648...), key empty (entire account record), but the oldValue bytes differ at 8 positions; newValue equals oldValue in both runs (the Vote tx didn’t change
those fields). This indicates the account record contents already diverged before this tx.

Why it differs

- Bandwidth path selection differs for this tx:
    - Embedded: path=ACCOUNT_NET, bytes=209
        - 2.embedded-java.log:78895–78897
    - Remote: ACCOUNT_NET insufficient (netLimit=0), fell back to path=FREE_NET, bytes=209
        - remote-java.df135ac.log:98617–98619
- The differing bytes in the account value match the resource usage counters:
    - Embedded oldValue has …d1…bd… which are 209 and 189 (ACCOUNT_NET=209 for this tx, FREE_NET=189 from the prior tx).
    - Remote oldValue has …00…018d… which is 0 and 397 (FREE_NET accumulated 189+209=398; serialized shows 0x018d=397, off-by-one likely a rounding/overhead difference).
    - This is consistent with embedded charging the Vote tx to ACCOUNT_NET and remote charging it to FREE_NET.
- Why remote thinks netLimit=0:
    - In remote mode, Java’s BandwidthProcessor computes path from its local DB state. The prior FreezeBalanceContract didn’t result in freeze/resource ledger state being applied to Java’s local stores (only
    an account byte blob was applied).
    - Evidence:
        - Freeze tx processing shows only 1 state change applied (an account update), not resource/freeze ledger changes: remote-java.df135ac.log:98266–98271
        - Rust backend config shows it’s not emitting extra storage changes for these ledgers and is in AEXT “hybrid” mode:
            - rust-backend/config.toml:101
            - remote-rust.df135ac.log:2
- Net effect: In remote mode, Java’s local view of the owner’s bandwidth limit is zero, so it always uses FREE_NET. That changes the resource usage counters in the account record, which changes the serialized
account bytes and thus the state digest at the first tx that touches this account again (the Vote).

Minimal byte-level confirmation

- Comparing oldValue bytes shows the fields that differ are exactly where the counters live:
    - Embedded window: 00414558540001004400000000000000 d1 00000000000000 bd 00000000000000
    - Remote window:   00414558540001004400000000000000 00 00000000000000 018d 00000000000000
    - 0xd1=209 (ACCOUNT_NET for Vote), 0xbd=189 (FREE_NET for Freeze), 0x018d=397≈189+209.

Pointers

- Embedded path selection: 2.embedded-java.log:78895
- Remote path selection and insufficiency: remote-java.df135ac.log:98618–98619
- Remote applying only 1 state change on Freeze: remote-java.df135ac.log:98266–98271
- Rust backend config (AEXT mode): rust-backend/config.toml:101
- Rust VoteWitness handling note: remote-rust.df135ac.log:11487–11498

Likely root cause

- Remote execution does not emit or apply freeze/resource ledger updates to Java’s local DB; Java’s BandwidthProcessor runs against incomplete local state and selects FREE_NET with netLimit=0.
- Additionally, the Rust backend’s bandwidth byte calculation shows “bandwidth=175” in some logs, which differs from Java’s 209; while secondary to this mismatch, it’s worth reconciling later.

How to fix/alleviate

- Enable emission and application of freeze/resource ledger changes in remote mode:
    - Set remote.execution.remote.emit_freeze_ledger_changes = true and make sure the backend returns AEXT/resource state updates.
    - In Java, ensure RuntimeSpiImpl applies those resource/AEXT changes to the correct stores, so BandwidthProcessor sees the updated netLimit immediately.
- Alternatively, set accountinfo_aext_mode = "tracked" so the backend tracks and emits AEXT deltas (net_usage, free_net_usage, limits) per tx; apply them on the Java side.
- Or, make Java defer bandwidth accounting to the remote backend in remote mode (i.e., consume and apply the backend’s authoritative resource counter updates instead of recomputing locally).

Next step

- Want me to tweak rust-backend/config.toml to “tracked”/emit changes and re-run a small window (blocks 2150–2155) to confirm the two CSVs align for this first mismatch?

---


Think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

Enable emission and application of freeze/resource ledger changes in remote mode:
- Set remote.execution.remote.emit_freeze_ledger_changes = true and make sure the backend returns AEXT/resource state updates.
- In Java, ensure RuntimeSpiImpl applies those resource/AEXT changes to the correct stores, so BandwidthProcessor sees the updated netLimit immediately.


---


• Here’s a concrete, code-aware plan to enable emission and application of freeze/resource ledger changes in remote mode, wired end-to-end so BandwidthProcessor sees updated netLimit immediately after freeze-
like txs.

Rust Backend (Emit Changes)

- Config toggle
    - Confirm flag exists and set it to true in rust-backend/config.toml:84 (emit_freeze_ledger_changes = true).
    - Ensure defaults are read from config at startup (already wired in crates/common/src/config.rs:220,238).
- Extend wire contract (proto)
    - Add a dedicated message for resource ledger mutations to framework/src/main/proto/backend.proto:
        - Message: FreezeLedgerChange { bytes owner_address; enum Resource { BANDWIDTH=0, ENERGY=1, TRON_POWER=2 } resource; int64 amount; int64 expiration_ms; }
        - Repeated field on ExecutionResult: repeated FreezeLedgerChange freeze_changes = <next_tag>;
        - Optional: Add GlobalResourceTotalsChange { int64 total_net_weight; int64 total_net_limit; int64 total_energy_weight; int64 total_energy_limit; } and expose repeated GlobalResourceTotalsChange
        global_resource_changes = <next_tag>; if you choose to emit dynamic properties updates atomically.
- Build changes at execution time
    - In crates/core/src/service.rs:
        - FreezeBalanceContract: execute_freeze_balance_contract(...) (note current comment “freeze ledger changes are not emitted” at rust-backend/crates/core/src/service.rs:1301) — branch on
        execution.remote.emit_freeze_ledger_changes.
            - After storage_adapter.add_freeze_amount(...), compute the delta/new record and push one FreezeLedgerChange for owner, resource, amount, expiration.
            - If you maintain dynamic totals in backend, compute and push one GlobalResourceTotalsChange.
        - Mirror the same emission pattern for other resource-affecting contracts: UnfreezeBalance(V1/V2), FreezeBalanceV2, DelegateResource, UndelegateResource, WithdrawExpireUnfreeze, CancelAllUnfreezeV2.
    - Conversion to proto response
        - In crates/core/src/service.rs where ExecuteTransactionResponse is built (see state conversion at rust-backend/crates/core/src/service.rs:3822), append freeze_changes (and global_resource_changes if
        used) into ExecutionResult.
- Optional AEXT parity improvements
    - If you need counters parity as well, keep accountinfo_aext_mode as “hybrid” or flip to “tracked” for authoritative counters (see rust-backend/crates/core/src/service.rs:3700–3860 for AEXT injection and
    the aext_map flow; config key at rust-backend/config.toml:101).

Java Apply Path (Consume Changes)

- Regenerate backend stubs
    - After proto update, regenerate gRPC classes used by Java (backend.proto lives in framework/src/main/proto/backend.proto).
- Surface changes in RemoteExecutionSPI
    - In framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:
        - In convertExecuteTransactionResponse(...) (around 700–900), parse protoResult.getFreezeChangesList().
        - Create a new internal DTO list (e.g., ExecutionSPI.FreezeLedgerChange) on the Java side and attach it to the returned ExecutionResult/ExecutionProgramResult (if needed, extend ExecutionProgramResult
        to carry these side-channel changes without disturbing existing stateChanges).
- Apply ledger in Runtime
    - In framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:
        - In execute(...), after executionSPI.executeTransaction(context) and before or within applyStateChangesToLocalDatabase(...), add a call to applyFreezeLedgerChanges(executionResult, context).
        - Implement applyFreezeLedgerChanges(...) to update the same stores Java actuators would update:
            - Fetch ChainBaseManager from context.getStoreFactory().getChainBaseManager().
            - Owner-level freeze
                - Load AccountCapsule and update Frozen (v1) or FrozenV2 (v2) lists based on resource in each change:
                    - V1 bandwidth: set/aggregate with setFrozenForBandwidth(long frozenBalance, long expireTime) at chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java:1008.
                    - V2 bandwidth/energy/tron_power: aggregate via helpers (addFrozenBalanceForBandwidthV2(...), addFrozenBalanceForEnergyV2(...), or update FreezeV2 entries via updateFrozenV2List(...), see
                    AccountCapsule at 545–566, 665–666, 941–952, 1095–1099, 1135–1147).
                - Persist via chainBaseManager.getAccountStore().put(address, accountCapsule) (as currently done for account state changes).
                - Mark dirty: ResourceSyncContext.recordAccountDirty(address).
            - Global resource totals (if emitted)
                - Apply with DynamicPropertiesStore setters:
                    - saveTotalNetWeight(...), saveTotalNetLimit(...), saveTotalEnergyWeight(...), saveTotalEnergyLimit2(...) as needed (see chainbase/src/main/java/org/tron/core/store/
                    DynamicPropertiesStore.java:1240–1345).
                - Mark dynamic keys dirty: ResourceSyncContext.recordDynamicKeyDirty("TOTAL_NET_WEIGHT".getBytes()), etc. The same keys are used by BandwidthProcessor logs.
            - Delegated resource ops (if later enabled): update DelegatedResourceStore (chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java) mirroring the standard actuator behavior for
            Delegate/Undelegate and locks.
        - Keep existing applyStateChangesToLocalDatabase(...) untouched for AccountInfo/AEXT application (it already deserializes AEXT and updates usage/window fields; see RuntimeSpiImpl.java:240–420).
- Ensure BandwidthProcessor reads updated state
    - Bandwidth path selection uses calculateGlobalNetLimit(accountCapsule) and checks accountCapsule.getAllFrozenBalanceForBandwidth() (see chainbase/src/main/java/org/tron/core/db/
    BandwidthProcessor.java:486 and 247 for diagnostics).
    - With freeze ledger applied before next tx in the same block, netLimit becomes > 0 and Java logs will show path=ACCOUNT_NET instead of FREE_NET.

Data Shapes and Parsing

- Freeze changes (owner-level)
    - Fields: owner_address (21-byte), resource (BANDWIDTH|ENERGY|TRON_POWER), amount (delta or absolute; recommend absolute to be idempotent), expiration_ms.
    - Java application: treat as “set/aggregate to new value” on the appropriate Frozen/FrozenV2 structure. If you choose “delta,” convert to absolute by reading current and adding to avoid double-application
    on retries.
- Global totals (optional)
    - Include total_net_weight, total_net_limit, total_energy_weight, total_energy_limit if you compute them in Rust; otherwise, Java can recompute and persist totals on demand.

Gating and Compatibility

- Behavior behind flag
    - When emit_freeze_ledger_changes=false, keep current behavior (single account-level state change only).
    - When true, emit freeze changes in addition to existing state changes; account-level change parity remains intact.
- Backward compatible Java
    - Code paths should no-op if freeze_changes field is absent (older backends).
    - Keep resource_usage array unused (it exists in proto but is not parsed yet; don’t repurpose it for ledger).

Validation Plan

- Re-run the window including:
    - The FreezeBalanceContract tx prior to block 2153.
    - The first VoteWitnessContract at block 2153 (tx 8ab04a...).
- Expected outcomes:
    - Remote Java logs show ACCOUNT_NET chosen (no “netLimit=0” or “ACCOUNT_NET insufficient”). File: remote-java.df135ac.log around the Vote tx line.
    - Remote CSV row for the Vote tx matches embedded for:
        - state_changes_json oldValue byte segment around AEXT (both show ...0041455854... d1 ... bd ..., not ... 00 ... 018d ...).
        - state_digest_sha256 identical to embedded.
- Optional: turn accountinfo_aext_mode=tracked to align counters even stronger; verify AEXT tail parity by diffing serialized AccountInfo segments in CSV.

Touch Points Summary

- Rust
    - rust-backend/config.toml:84
    - rust-backend/crates/core/src/service.rs: execute_freeze_balance_contract and peers; response builder section near 3822
    - framework/src/main/proto/backend.proto: add FreezeLedgerChange (+ optional GlobalResourceTotalsChange)
- Java
    - framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java: convertExecuteTransactionResponse(...) to parse freeze_changes
    - framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java: add applyFreezeLedgerChanges(...) hook; use AccountCapsule and DynamicPropertiesStore setters
    - chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java: relies on updated state; no code change needed

If you want, I can also outline the exact field encoding for the freeze change payloads and the Java-side update logic for each resource/contract type to keep it 1:1 with the embedded actuators’ effects.


---


