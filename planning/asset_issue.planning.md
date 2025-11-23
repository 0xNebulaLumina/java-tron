No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

- Add mapping in Java for TRC-10 contracts and implement the handlers in Rust:
    - Java: add AssetIssueContract in RemoteExecutionSPI switch with proper ContractType, TxKind=NON_VM, and payload fields.
    - Rust: implement AssetIssueContract handlers in `execute_non_vm_contract`

BTW, we probably also need to:
1. emit account changes due to AssetIssueContract, send them back to java, java apply to accountStore and flush to db
2. if there's any pre-exec effect to the account/dynamicProperty store (due to fee/bandwidth/energy), we also need to flush to db before executing tx
3. even there's not tx, if there's any change to the account/dynamicProperty store (due to fee/bandwidth/energy), we still need to flush to db


---


• Goal

- Add TRC-10 AssetIssueContract mapping on the Java remote SPI and implement the non‑VM handler in Rust, with the right TxKind/ContractType and payload wiring.
- Ensure account deltas (fee/bandwidth/AEXT) flow back to Java and get applied to AccountStore; plan for flushing dynamic properties where applicable.

Repo Understanding (where changes land)

- Java mapping: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:buildExecuteTransactionRequest(...)
- gRPC proto: framework/src/main/proto/backend.proto
- Rust entry: rust-backend/crates/core/src/service/grpc/mod.rs → dispatches to execute_non_vm_contract in rust-backend/crates/core/src/service/mod.rs:194
- Rust parsing utilities: rust-backend/crates/core/src/service/contracts/proto.rs
- AEXT/account changes conversion: Rust → Java via rust-backend/crates/core/src/service/grpc/conversion.rs:174
- Java apply: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java (applies AccountChange, Freeze changes, Global resource totals)

Plan — Java Mapping (RemoteExecutionSPI)

- Add AssetIssueContract case to the contract-type switch
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java
    - Behavior:
        - Set txKind = TxKind.NON_VM
        - Set contractType = tron.backend.BackendOuterClass.ContractType.ASSET_ISSUE_CONTRACT
        - fromAddress comes from getOwnerAddress() (already set)
        - toAddress = new byte[0] (system contract, no recipient)
        - data = contractParameter.unpack(AssetIssueContractOuterClass.AssetIssueContract.class).toByteArray() (send full proto bytes for Rust parsing)
        - Do not set assetId
    - Gating:
        - Reuse existing TRC‑10 toggle for safety, or add a finer-grained JVM flag:
            - Preferred: reuse -Dremote.exec.trc10.enabled=true
            - Optional granular: -Dremote.exec.trc10.asset_issue.enabled=true
        - If disabled, throw UnsupportedOperationException to fall back to Java actuators (mirrors TransferAsset behavior at framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:309)
- Ensure pre-exec AEXT snapshots include owner only (already collected; see framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:876) — no change required.

Plan — Rust Handler (execute_non_vm_contract)

- Dispatch arm
    - File: rust-backend/crates/core/src/service/mod.rs
    - Add match arm in execute_non_vm_contract:
        - Some(tron_backend_execution::TronContractType::AssetIssueContract) => { ... }
        - Gate behind config flag(s):
            - Use execution.remote.trc10_enabled initially.
            - Optionally add asset_issue_enabled for finer control (see “Config changes”).
- Implement Asset Issue handler
    - New module: rust-backend/crates/core/src/service/contracts/trc10.rs (or extend existing contracts module)
    - Responsibilities (Phase 1, minimal parity):
        - Parse minimal fields from transaction.data (full proto bytes):
            - Name (bytes), total_supply (int64), trx_num, num, start_time, end_time, precision.
            - Use read_varint + simple field‐tag scanning similar to freeze parsers (see rust-backend/crates/core/src/service/contracts/freeze.rs:980).
            - Do not attempt full validation against Asset stores in Phase 1 (storage adapter lacks TRC‑10 stores).
        - Read dynamic properties:
            - Asset issue fee from properties key ASSET_ISSUE_FEE (see chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:1554, 1568).
            - Blackhole optimization and blackhole address (already implemented: engine.rs:321, 407).
        - Validate balance ≥ asset issue fee; return Err if insufficient.
        - Apply fee:
            - Deduct fee from owner; emit TronStateChange::AccountChange for owner (old→new).
            - If not burning, credit blackhole account; emit AccountChange for blackhole.
            - Persist both via storage_adapter.set_account(...) to keep Rust storage consistent for subsequent tx.
        - Bandwidth/AEXT tracking:
            - Compute bandwidth_used = calculate_bandwidth_usage(transaction) (as done elsewhere).
            - If accountinfo_aext_mode == "tracked", update owner’s AEXT using ResourceTracker (see rust-backend/crates/core/src/service/mod.rs:1125+ and for transfer path at rust-backend/crates/core/src/
            service/mod.rs:318).
            - Populate aext_map to echo back in Java’s AccountInfo deserialization.
        - Sort state_changes deterministically by address for CSV parity (mirror witness create; rust-backend/crates/core/src/service/mod.rs:704).
        - Return TronExecutionResult with energy_used = 0, bandwidth_used, state_changes, aext_map.
    - Phase 2 (follow-up):
        - Validation logic: enforce name rules with getAllowSameTokenName (key is " ALLOW_SAME_TOKEN_NAME" with a leading space; see chainbase/src/main/java/org/tron/core/store/
        DynamicPropertiesStore.java:118, 1953) and duplicate checks via storage (requires adding asset stores to storage adapter).
        - Token ID management: read and increment TOKEN_ID_NUM (chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:980), persist new value.

Plan — Config Changes

- Option A (simpler): reuse execution.remote.trc10_enabled to guard AssetIssueContract.
- Option B (granular): add execution.remote.asset_issue_enabled to RemoteExecutionConfig:
    - File: rust-backend/crates/common/src/config.rs
        - Add pub asset_issue_enabled: bool to RemoteExecutionConfig.
        - Default false in both Default impl and builder defaults.
    - Config file: rust-backend/config.toml
        - Document and set default asset_issue_enabled = false.

Plan — Proto and Result Plumbing (optional Phase 2 for full TRC‑10 parity)

- To mirror Java Actuator behavior (create AssetIssue[V1/V2] entries, update account asset maps, bump TOKEN_ID_NUM) without implementing all TRC‑10 stores in Rust, extend the gRPC result to carry TRC‑10
semantic changes back to Java for application:
    - backend.proto additions:
        - Message Trc10Change with oneof variants:
            - AssetIssued { name, total_supply, precision, start_time, end_time, trx_num, num, owner_address, token_id (v2) }
            - Future: Trc10Transferred, Trc10Participated, Trc10Updated
        - Add repeated Trc10Change trc10_changes = <next_field>; to ExecutionResult.
    - Rust: populate AssetIssued with parsed fields and computed token_id (if Phase 2 implements ID bump).
    - Java: extend RemoteExecutionSPI.convertExecuteTransactionResponse(...) to parse trc10_changes and:
        - Create AssetIssueCapsule in AssetIssueStore and AssetIssueV2Store.
        - Update issuer AccountCapsule asset maps (V1 by name; V2 by id).
        - Increment and persist TOKEN_ID_NUM in DynamicPropertiesStore.
    - This keeps Rust focused on compute/validation and uses Java stores for TRC‑10 persistence out of the gate.

Plan — Java Apply Changes and Flushing

- Account changes:
    - Already applied by RuntimeSpiImpl.applyStateChangesToLocalDatabase(...) which updates balances and AEXT (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:520+ and 544+).
- Dynamic properties and pre‑exec resource states:
    - Global resource totals are already handled via GlobalResourceTotalsChange (framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:584+).
    - For bandwidth and AEXT: ensure backend config sets accountinfo_aext_mode = "hybrid" (default in rust-backend/config.toml) so AEXT is echoed and Java writes these fields to AccountStore (flush happens
    via AccountStore.put).
- Even when there is “no tx”:
    - If we later emit isolated resource/dynamic property changes (e.g., through block maintenance hooks), they should be carried in the same state change channel; Java will apply and put/save* flushes to db.

Plan — Tests

- Java
    - Add a focused test to validate classification:
        - Build a Transaction with AssetIssueContract and assert:
            - TxKind == NON_VM
            - ContractType == ASSET_ISSUE_CONTRACT
            - Payload bytes exactly match contract.toByteArray()
        - File: framework/src/test/java/org/tron/common/client/ExecutionGrpcClientTest.java or new SPI test.
- Rust
    - Unit tests for handler:
        - Given owner balance ≥ asset issue fee, executing AssetIssue returns:
            - 2 account changes (owner −fee, blackhole +fee) when not burning
            - 1 account change (owner −fee) when burning
            - energy_used = 0, bandwidth_used equals serialized payload size cost
            - AEXT tracked changes are persisted and echoed when accountinfo_aext_mode = "tracked"
        - Disabled flag test: when trc10_enabled (or asset_issue_enabled) is false, return an error to trigger Java fallback.
- Integration
    - End‑to‑end path (optional): enable flags, run a simple AssetIssue through the GRPC layer, inspect ExecuteTransactionResponse state_changes.

Plan — Edge Cases and Rollout

- Default to disabled for safety:
    - Java path: require -Dremote.exec.trc10.enabled=true (or dedicated flag)
    - Rust config: keep execution.remote.trc10_enabled = false (and optional asset_issue_enabled = false)
- Validation that’s out‑of‑scope in Phase 1:
    - Name uniqueness against AssetIssueStore (ALLOW_SAME_TOKEN_NAME = 0)
    - Creation of AssetIssue[V1/V2] entries and account asset maps
    - TOKEN_ID_NUM bump
    - Plan to deliver via proto extension in Phase 2 so Java stores perform the writes.
- Determinism:
    - Keep deterministic ordering of state changes (sort by address)
    - Avoid adding storage changes unless expressly enabled to keep CSV parity toggles stable.

Summary of Concrete Changes

- Java
    - RemoteExecutionSPI.java: add AssetIssueContract case with NON_VM, ASSET_ISSUE_CONTRACT, data = contract.toByteArray(), gated by a TRC‑10 flag.
- Rust
    - core/service/mod.rs: new match arm for AssetIssueContract under execute_non_vm_contract.
    - core/service/contracts/trc10.rs: parse proto bytes, compute and emit fee deltas, bandwidth, AEXT tracking; persist owner/blackhole accounts.
    - execution/storage_adapter/engine.rs: add helpers:
        - get_asset_issue_fee() reading "ASSET_ISSUE_FEE"
        - (Phase 2) get_allow_same_token_name(), get_token_id_num(), set_token_id_num()
    - common/config.rs (+config.toml): optionally add asset_issue_enabled flag in RemoteExecutionConfig.

Want me to draft the exact switch case for Java and the Rust handler skeleton with parsing stubs and state change emission next?