No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

- Add mapping in Java for TRC-10 contracts and implement the handlers in Rust:
    - Java: add AssetIssueContract/ParticipateAssetIssueContract in RemoteExecutionSPI switch with proper ContractType, TxKind=NON_VM, and payload fields.
    - Rust: set execution.remote.trc10_enabled=true in rust-backend/config.toml and implement TRC-10 handlers (transfer is stubbed; asset issue/participate are unimplemented).

---


• Here’s a precise, end‑to‑end plan to add the Java TRC‑10 mappings and implement the Rust handlers, with gating to keep rollout safe and file‑level guidance for each change.

Scope

- Java: extend remote execution mapping to include AssetIssueContract and ParticipateAssetIssueContract as NON_VM, with correct ContractType and payloads.
- Rust: enable TRC‑10 in config; wire contract dispatch; implement Asset Issue and Participate handlers against the unified storage (RocksDB) with the minimal DB keys to match java‑tron stores; keep
TransferAssetContract as stub for now.

Java Changes

- RemoteExecution mapping
    - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:296
    - Add imports:
        - org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract
        - org.tron.protos.contract.AssetIssueContractOuterClass.ParticipateAssetIssueContract
    - Add cases to the switch in buildExecuteTransactionRequest:
        - AssetIssueContract
            - Gate with Boolean.parseBoolean(System.getProperty("remote.exec.trc10.enabled", "false")); if false, throw UnsupportedOperationException to fall back.
            - Unpack with contractParameter.unpack(AssetIssueContract.class).
            - Set: fromAddress = context owner; toAddress = empty; value = 0; data = full assetIssue.toByteArray(); txKind = NON_VM; contractType =
            tron.backend.BackendOuterClass.ContractType.ASSET_ISSUE_CONTRACT; assetId empty.
        - ParticipateAssetIssueContract
            - Same gate as above.
            - Unpack with contractParameter.unpack(ParticipateAssetIssueContract.class).
            - Set: fromAddress = owner; toAddress = to_address; value = 0; data = full participate.toByteArray(); txKind = NON_VM; contractType =
            tron.backend.BackendOuterClass.ContractType.PARTICIPATE_ASSET_ISSUE_CONTRACT; assetId = asset_name bytes.
    - Pre‑exec AEXT snapshots
        - File: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:902
        - Extend collectPreExecutionAext() to also include toAddress for ParticipateAssetIssueContract, mirroring TransferAssetContract behavior (helps CSV parity on bandwidth calculations).
- Proto: ensure enum consistency
    - File: framework/src/main/proto/backend.proto:501
    - Confirm ContractType already includes ASSET_ISSUE_CONTRACT and PARTICIPATE_ASSET_ISSUE_CONTRACT (present).

Rust Changes

- Enable feature flag
    - File: rust-backend/config.toml
    - Set: [execution.remote] trc10_enabled = true.
    - Optional: log this in startup
        - File: rust-backend/src/main.rs:52–58
        - Add a log line: info!("  TRC-10 enabled: {}", config.execution.remote.trc10_enabled);.
- Wire contract dispatch
    - File: rust-backend/crates/core/src/service/mod.rs:200–320
    - Add match arms:
        - Some(TronContractType::AssetIssueContract) → if !remote_config.trc10_enabled return Err("... disabled ..."); else call self.execute_asset_issue_contract(...).
        - Some(TronContractType::ParticipateAssetIssueContract) → same gating; call self.execute_participate_asset_issue_contract(...).
        - Leave TransferAssetContract as is (stubbed with clear error string).
- Parse TRC‑10 contract payloads (prost)
    - Add a small crate or module to compile the TRON contract protos needed:
        - Source protos:
            - protocol/src/main/protos/core/contract/asset_issue_contract.proto:9
        - Crate: rust-backend/crates/protos (new), compile with prost-build for at least:
            - protocol.AssetIssueContract
            - protocol.ParticipateAssetIssueContract
        - Wire into core/execution crates via Cargo dependencies to decode transaction.data in handlers.
- Storage adapter extensions (unified RocksDB)
    - Keys and DB names must match java‑tron:
        - AssetIssueStore V1 DB name: "asset-issue" (name‑keyed)
        - AssetIssueV2Store DB name: "asset-issue-v2" (id‑keyed)
        - AccountAssetStore DB name: "account-asset" (key: 21‑byte account + assetID)
        - DynamicPropertiesStore DB name: "properties"
    - Extend EngineBackedEvmStateStore:
        - File: rust-backend/crates/execution/src/storage_adapter/engine.rs
        - Add helpers:
            - get_allow_same_token_name(): read key " ALLOW_SAME_TOKEN_NAME" (note the leading space used in java class), default 0 if absent; expose both raw and typed i64.
            - get_asset_issue_fee(): read "ASSET_ISSUE_FEE", default to a sane test value (or 0) if missing.
            - get_token_id_num()/save_token_id_num(): read/write "TOKEN_ID_NUM".
            - get/set_account_asset_balance(address, asset_id): DB "account-asset"; key = address(21 bytes) + asset_id (bytes).
            - put_asset_issue_v1(name_key, bytes), put_asset_issue_v2(id_key, bytes)
        - For TRX balance adjustments, reuse existing account read/write via AccountInfo and set_account (preserve other fields).
- Implement Asset Issue
    - File: rust-backend/crates/core/src/service/mod.rs
    - Add function execute_asset_issue_contract(&self, store, tx, ctx) -> Result<TronExecutionResult, String>:
        - Decode protocol::AssetIssueContract from tx.data.
        - Validate (match java‑tron minimal set):
            - owner address exists; token name valid; url/description valid; start_time < end_time and > block header time (use ctx.block_timestamp); total_supply > 0; trx_num > 0; num > 0; frozen supplies
            sum <= total_supply; frozen_days within min/max in properties (if not implemented yet, accept and log TODO).
            - deny "trx" as name when AllowSameTokenName != 0.
        - Allocate new id: read TOKEN_ID_NUM, increment, write back; set id into the contract bytes (prost builder).
        - Precision rule: if AllowSameTokenName == 0, set V2 precision = 0 to match java behavior.
        - Persist:
            - If AllowSameTokenName == 0: write V1 "asset-issue" with name key and V2 "asset-issue-v2" with id key.
            - Else: write only "asset-issue-v2".
        - Asset allocation to owner:
            - Compute remain_supply = total_supply - sum(frozen_amount).
            - Put remain_supply into AccountAssetStore (account-asset) as assetID (V2 id); if AllowSameTokenName == 0, optionally also reflect name‑keyed balance if needed later (can be deferred).
        - Fee handling:
            - Subtract ASSET_ISSUE_FEE from owner TRX; credit blackhole or burn based on support_black_hole_optimization() (already implemented).
        - State emission:
            - Build one AccountChange for owner (old == new for CSV parity is OK), bandwidth_used via existing calculator.
            - Return TronExecutionResult success, energy_used=0.
- Implement Participate Asset Issue
    - File: rust-backend/crates/core/src/service/mod.rs
    - Add function execute_participate_asset_issue_contract(&self, store, tx, ctx) -> Result<TronExecutionResult, String>:
        - Decode protocol::ParticipateAssetIssueContract from tx.data.
        - Validate:
            - owner and to addresses valid and not equal; amount > 0.
            - Fetch asset: Select store based on AllowSameTokenName, key = asset_name bytes. If not found, error.
            - Confirm toAddress matches asset owner.
            - Time window: ctx.block_timestamp within [start_time, end_time).
            - Compute exchange_amount = floor(amount * num / trx_num). If <= 0, error.
            - Check issuer (toAddress) asset balance >= exchange_amount in AccountAssetStore; Check owner TRX balance >= amount (plus any fee=0 per java).
        - Apply:
            - Debit owner TRX by amount; credit issuer TRX by amount.
            - Credit owner asset balance (account-asset) by exchange_amount.
            - Debit issuer asset balance (account-asset) by exchange_amount.
        - State emission:
            - Two AccountChange entries (owner, issuer), old==new acceptable.
            - Bandwidth via existing calculator; energy_used=0.
- Leave TRC‑10 Transfer stubbed
    - Keep the existing Err("TRC‑10 transfers not yet implemented...") so Java can fall back if mapping accidentally routes it.
- Tests (Rust)
    - File: rust-backend/crates/core/src/service/tests/contracts.rs
    - Add unit tests for:
        - AssetIssue happy path: writes asset‑issue‑v2, increments token id, credits issuer’s account‑asset balance, debits TRX fee correctly to blackhole/burn; returns success and 1 account change; uses
        ctx.block_timestamp for validation.
        - Participate: validates time window and balances; performs the four balance deltas; returns success and 2 account changes.
    - Use temporary StorageEngine in tempdir; set required dynamic properties keys prior to run (ALLOW_SAME_TOKEN_NAME, ASSET_ISSUE_FEE, TOKEN_ID_NUM).
- Optional Java apply path (only if needed later)
    - If we prefer to emit TRC‑10 changes rather than persist in Rust, we could extend backend.proto with a Trc10LedgerChange repeated field and apply changes on Java side (similar to freeze changes), but
    this plan chooses direct persistence in Rust to match RemoteStorage mode and reduce Java code changes.

Validation

- Unit tests
    - ./gradlew :framework:test --tests "*Remote*IntegrationTest" as sanity if exists.
    - New Rust tests under core/service for asset issue and participate pass.
- Manual wiring
    - Start backend: cd rust-backend && cargo run --release and confirm logs print TRC-10 enabled: true.
    - Run a small Java node against remote backend using -Dremote.exec.trc10.enabled=true and create/test TRC‑10 issuance and participation via RPC:
        - CreateAssetIssue2, ParticipateAssetIssue2; verify balances via Wallet APIs (e.g., GetAssetIssueById, account balances).

Risks & Mitigation

- Proto parsing in Rust:
    - Mitigate by generating prost from existing TRON protos; avoid hand‑rolled decoders.
- Account serialization:
    - We only use AccountInfo for TRX balance adjustments; asset balances are stored in account-asset DB; this avoids needing to round‑trip the full Account proto.
- DB key parity:
    - Match DB names/keys exactly: "asset-issue", "asset-issue-v2", "account-asset", "properties".
- Gating:
    - Java mapping is gated by -Dremote.exec.trc10.enabled; Rust execution is gated by execution.remote.trc10_enabled. Both must be true to use remote path; otherwise fall back to Java actuators.

Deliverables

- Java: RemoteExecutionSPI mapping + optional AEXT snapshot tweak.
- Rust:
    - config.toml toggle
    - dispatch arms + handlers
    - storage adapter extensions
    - prost‑generated TRON contract protos
    - targeted unit tests

If you want, I can start by wiring the Java switch cases and add the Rust dispatch arms, then scaffold the prost crate and storage helpers before implementing the handlers.


---


