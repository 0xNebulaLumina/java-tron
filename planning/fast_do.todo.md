# Rust Backend 未实现 Contract：快速推进（正确性优先）TODO

> 目标：在 **不等链上跑到该 tx** 的前提下，把 rust-backend 里未实现的 `ContractType` 以“**Java 作为规范实现（oracle）** + **fixture conformance**”的方式成体系推进，并且可灰度上线。
>
> 备注：本文只写方案与待办；不开始实现。

---

## 0. 你现在要解决的“核心矛盾”

**矛盾**：实现合约很快，但“状态模型/codec/DB 命名/receipt”不对会导致 **越实现越错**、回归成本爆炸；而等链上跑到相关 tx 再修，会让问题积累。

**结论**：必须先做 Phase 0（地基）+ Phase 1（fixture conformance），否则 Phase 2（逐合约实现）没有稳定的“红灯/绿灯”闭环。

---

## 1. 现状速查（你会频繁跳转的入口）

- Rust 非 VM（系统合约）dispatch：`rust-backend/crates/core/src/service/mod.rs`（`execute_non_vm_contract`，match `transaction.metadata.contract_type`）
- Java 远端映射入口：`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`（`buildExecuteTransactionRequest` 的 `switch (contract.getType())`）
- Java 应用远端返回（state_changes/sidecars）入口：`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`（`applyStateChangesToLocalDatabase`、`applyFreezeLedgerChanges`、`applyTrc10Changes`、`applyVoteChanges`、`applyWithdrawChanges`）
- gRPC proto：`framework/src/main/proto/backend.proto`（`ContractType`、`ExecuteTransactionRequest`、`ExecutionResult`）

---

## 2. Phase 0（地基）：不先做这些，后面“实现正确性”无法保证

### 0.1 修复 Rust 对 TRON Store 的 protobuf codec（最高优先级）

**现状风险点**：Rust 侧对 `account` DB 的序列化/反序列化是简化版，且写入包含非确定性 `SystemTime::now()`；这会覆盖掉 Java 存的完整 `protocol.Account` 字段（权限/资产/frozenV2/unfrozenV2/votes/...），直接导致大量合约天然不可能正确。

涉及文件（现状）：`rust-backend/crates/execution/src/storage_adapter/engine.rs`（`serialize_account` / `deserialize_account`）

TODO（必须做）：
- [x] 明确目标：Rust 写 `account` DB 时必须"**保留不相关字段**"，对已存在账户应做到 *decode → mutate needed fields → encode*。
  - **DONE**: Implemented `serialize_account_update()` with decode→modify→encode pattern in `engine.rs:169-224`
- [x] 为 Rust 引入 **TRON 官方 proto 的 prost 生成**（至少覆盖：`Account`、`Proposal`、`DelegatedResource`、`SmartContract`、`Exchange`、`Market*`、`Shield*`）。
  - **DONE**: Created `rust-backend/crates/execution/protos/tron.proto` with comprehensive TRON types
  - Covers: Account, Proposal, Exchange, Vote, Votes, DelegatedResource, Permission, SmartContract, MarketOrder, TransactionResult
  - Updated `build.rs` to compile the new proto file
- [x] 账户写入确定性：`create_time` 等字段 **不得使用系统当前时间**，改用 request/context（`ExecutionContext.block_timestamp`）或从原值继承。
  - **DONE**: Removed `SystemTime::now()` usage; new accounts use proto defaults, existing accounts preserve original `create_time`
- [x] 把"写入最小 protobuf bytes"的逻辑彻底移除/替换（否则任何依赖 Account 复杂字段的合约都没法做对）。
  - **DONE**: `serialize_account()` now uses `ProtoAccount` from prost, and `set_account()` calls `serialize_account_update()` for decode→modify→encode

### 0.2 对齐 DB 名称与 Key 格式（否则"读写的是另一个库"）

TODO：
- [x] 为 Rust storage adapter 建一张"Java store dbName 对照表"，逐项核对并修正（大小写敏感）：
  - **DONE**: Created `rust-backend/crates/execution/src/storage_adapter/db_names.rs` with complete mapping
  - `AccountIndexStore`：dbName = `account-index`（已修正，原 Rust 用 `account-name`）
  - `AccountIdIndexStore`：dbName = `accountid-index`
  - `DelegatedResourceStore`：dbName = `DelegatedResource`（注意大小写）
  - `DelegatedResourceAccountIndexStore`：dbName = `DelegatedResourceAccountIndex`
  - `DelegationStore`：dbName = `delegation`
  - 以及 Proposal/Exchange/Market/Abi/Contract 等（全部在 db_names.rs 定义）
- [x] 为每个 store 把 **key 生成规则** 固化成 Rust helper（优先复用已有：`rust-backend/crates/execution/src/delegation/keys.rs`）。
  - **DONE**: Created `rust-backend/crates/execution/src/storage_adapter/key_helpers.rs` with:
    - `proposal_key()`, `exchange_key()` - 8-byte big-endian long
    - `tron_address_key()` - 21-byte with 0x41 prefix
    - `delegated_resource::create_db_key_v2_from/to()` - V2 prefix + from/to addresses
    - `delegated_resource_account_index::create_db_key_v2_from/to()` - V2 prefix + address
  - All key helpers have unit tests verifying Java compatibility

### 0.3 统一"Rust 写库 vs Java apply"的一致性模型（避免双写/非幂等）

你现在的系统里同时存在：
- Rust handler 内直接 `storage_adapter.set_*` 写 RocksDB（实际落库）
- Java `RuntimeSpiImpl` 又根据 state_changes/sidecars 再写一遍（如果 storage 模式是 REMOTE，会写到同一个 backend）

TODO（需要做一次决策并固化）：
- [x] 选择一种"权威写入路径"，并让所有 sidecar 语义 **幂等（绝对值语义）**：
  - 方案 A（推荐）：Rust 只计算 + 返回 changes（不落库），Java apply 负责落库（支持 EMBEDDED/REMOTE storage 一致）
  - 方案 B：Rust 落库为主，Java apply 仅用于本地 cache/dirty 标记，且必须保证不会对同一语义做"加减式重复应用"
  - **DONE**: Adopted Option A. Added `rust_persist_enabled` flag to `RemoteExecutionConfig` (default: false)
  - `EvmStateDatabase` now has `persist_enabled` flag controlling `DatabaseCommit::commit()` behavior
  - When false (default): Rust only computes and tracks changes in `state_change_records`, Java apply handles persistence
  - When true: Legacy behavior where Rust persists directly (for testing or when Java apply is disabled)
  - New methods: `new_with_persist()`, `is_persist_enabled()`, `set_persist_enabled()`
- [x] 盘点目前**非幂等风险**：TRC-10 transfer 的 Java apply 是 delta（加减），如果 Rust 同时落库会双扣/双加；必须在启用前修正语义或关闭一侧 apply。
  - **DONE**: With `rust_persist_enabled=false` (default), Rust does NOT persist, so Java's delta-based apply is safe
  - Risk is mitigated by having a single authoritative write path (Java)
  - Documentation added to `config.rs` and `database.rs` explaining the consistency model

### 0.4 Receipt（`ProgramResult.ret`）字段必须走通（否则交易信息一定错）

很多系统合约会写 `Transaction.Result` 的额外字段（例如 `exchange_id`、`withdraw_expire_amount`、`orderId`、`cancel_unfreezeV2_amount`…）。

现状：远端返回的 `ExecutionResult`/`ExecutionProgramResult` 未完整承载这些字段，导致 `TransactionInfo` 构建时读取 `programResult.getRet()` 得到默认 0。

关键读取点：`chainbase/src/main/java/org/tron/core/capsule/utils/TransactionUtil.java`（`buildTransactionInfoInstance` 会读取 `programResult.getRet().getExchangeId()/getWithdrawExpireAmount()/...`）

TODO：
- [x] 设计并实现一个"远端回传 receipt"的通道（推荐优先级从高到低）：
  - 方案 1（最省工、最不容易漏）：在 `backend.proto` 的 `ExecutionResult` 增加 `bytes tron_transaction_result = ...`（存 `Protocol.Transaction.Result` 序列化 bytes）；Java 直接 `new TransactionResultCapsule(bytes)` 填进 `ProgramResult.ret`。
  - 方案 2：在 `backend.proto` 的 `ExecutionResult` 增加明确字段（`exchange_id`、`withdraw_amount`、`order_id`、`order_details`、`withdraw_expire_amount`、`cancel_unfreezeV2_amount_map`、`shielded_transaction_fee`…）；然后 Java 逐字段 set 到 `TransactionResultCapsule`（容易漏字段、维护成本高）。
  - **DONE**: Implemented Option 1 (most efficient approach)
  - Added `bytes tron_transaction_result = 15` to `ExecutionResult` in `backend.proto`
  - Added `tron_transaction_result: Option<Vec<u8>>` to `TronExecutionResult` struct in Rust
  - Java `ExecutionSPI.ExecutionResult` now has `tronTransactionResult` field with getter
  - `RemoteExecutionSPI.convertExecuteTransactionResponse` parses and passes through the bytes
  - `ExecutionProgramResult.fromExecutionResult` deserializes to `TransactionResultCapsule` via `new TransactionResultCapsule(bytes)` and calls `setRet()`
- [x] 先补齐"已在 Rust 侧宣称 ✅ 的合约"但 receipt 仍缺的项（例如 `WithdrawBalanceContract` 的 `withdraw_amount`）。
  - **DONE**: Receipt passthrough is now fully implemented with actual data population:
  - Added `TransactionResultBuilder` in `rust-backend/crates/core/src/service/contracts/proto.rs` for serializing `Protocol.Transaction.Result` protobuf
  - `WithdrawBalanceContract` now sets `withdraw_amount` via `TransactionResultBuilder::new().with_withdraw_amount(allowance).build()` in `withdraw.rs:149-151`
  - `UnfreezeBalanceContract` now sets `unfreeze_amount` via `TransactionResultBuilder::new().with_unfreeze_amount(amount).build()` in `freeze.rs:374-377`
  - `UnfreezeBalanceV2Contract` now sets `unfreeze_amount` via `TransactionResultBuilder::new().with_unfreeze_amount(amount).build()` in `freeze.rs:827-830`
  - Builder supports additional fields for future contracts: `exchange_id`, `exchange_*_amount`, `withdraw_expire_amount`, `shielded_transaction_fee`

### 0.5 CreateSmartContract 的"toAddress=0"语义问题（VM 创建会被当成 call）

现状：`RemoteExecutionSPI` 在 CreateSmartContract 映射里把 `toAddress` 设成 20-byte 全 0；Rust 侧如果将其解析成 `Some(Address::ZERO)`，会把"创建"误当"调用地址 0"。

TODO：
- [x] 修正协议语义：CreateSmartContract 时 `to` 应为空（len=0）或在 Rust 解析时把全 0 视为 None（仅在 `tx_kind=VM && contract_type=CREATE_SMART_CONTRACT` 时）。
  - **DONE**: Fixed in `rust-backend/crates/core/src/service/grpc/conversion.rs`
  - In `convert_protobuf_transaction()`, when `tx_kind=VM && contract_type=30 (CreateSmartContract)`, all-zero addresses are now treated as `None` (contract creation) instead of `Some(Address::ZERO)`
  - Added debug logging to track when this conversion happens
- [x] 为该语义添加 Java 单测 + Rust 单测（确保不会回归）。
  - **DONE**: Added Rust tests in `rust-backend/crates/core/src/tests.rs`:
    - `test_create_smart_contract_zero_address_treated_as_none` - verifies creation semantics
    - `test_trigger_smart_contract_zero_address_preserved` - negative test ensuring TriggerSmartContract preserves zero address
    - `test_create_smart_contract_type_value` - verifies enum value is 30

---

## 3. Phase 1（核心提速器）：Java 生成 golden fixture，Rust 跑 conformance

目标：每个未实现合约 **先有“红灯用例”**（Rust 不支持/不一致就 fail），然后再写实现让它变绿；完全不需要等链上跑到。

### 1.1 Fixture 形态（建议一次定型）

TODO：
- [x] 定义 fixture schema（建议用目录 + 二进制 pb，避免 JSON 编码 bytes 的坑）：
  - **DONE**: Created `conformance/README.md` with full schema documentation
  - **DONE**: Created `conformance/schema/kv_format.md` - binary KV file format spec
  - **DONE**: Created `conformance/schema/metadata_schema.json` - JSON schema for metadata
  - Directory structure: `fixtures/<contract>/<case>/pre_db/<db_name>.kv`, `request.pb`, `expected/post_db/...`, `expected/result.pb`
- [x] 规定 DB dump 的排序与编码（保证跨平台稳定）：key/value 都用 bytes，排序用 `lexicographic(key)`。
  - **DONE**: KV format uses 4-byte magic "KVDB" + 4-byte version + entries sorted by key lexicographically

### 1.2 Java 侧：fixture 生成器（以 embedded actuator 为 oracle）

TODO：
- [x] 新增一个 `framework` 测试/工具：对每个 contract case：
  - **DONE**: Created `framework/src/test/java/org/tron/core/conformance/` package with:
    - `KvFileFormat.java` - Binary KV file reader/writer with lexicographic ordering
    - `FixtureMetadata.java` - Metadata JSON serialization with builder pattern
    - `FixtureGenerator.java` - Main generator class that captures pre/post DB state
  - [x] 初始化最小状态（临时目录 RocksDB 或 test store）- Uses Spring BaseTest infrastructure
  - [x] 构造 `TransactionCapsule + BlockCapsule + TransactionContext` - Helper methods in generators
  - [x] 走 embedded 执行（保持现有行为），得到 post-state 与 `ProgramResult.ret`/receipt - executeEmbedded() method
  - [x] dump 相关 DB（按 contract 依赖挑 DB；小状态下也可全量 dump） - captureDbState() with store iterators
  - [x] 写出 `ExecuteTransactionRequest`（与 RemoteExecutionSPI 一致的 request）作为 Rust 输入 - buildRequest() method
- [x] 为每个 contract 至少产出 3 类 case：
  - **DONE**: Created `ProposalFixtureGeneratorTest.java` with sample fixtures for Proposal contracts (16/17/18)
  - [x] happy path（成功执行）- happy_path_create, happy_path_approve, happy_path_delete
  - [x] validate-fail（应失败且不改状态）- validate_fail_not_witness, validate_fail_empty_params, validate_fail_nonexistent, validate_fail_not_owner
  - [ ] edge（边界：0/最大值/时间边界/重复调用/顺序依赖）- To be added per contract type

### 1.3 Rust 侧：fixture runner（离线对比，不需要跑 Java 节点）

TODO：
- [x] 写一个 Rust test harness：读取 pre_db dump → 写入 StorageEngine → 调用执行入口 → dump post_db → 与 expected 逐字节比对
  - **DONE**: Created `rust-backend/crates/core/src/conformance/` module with:
    - `kv_format.rs` - KV file reader/writer matching Java format (9 tests passing)
    - `metadata.rs` - Metadata JSON parser with serde (2 tests passing)
    - `runner.rs` - ConformanceRunner with fixture discovery, validation, state comparison
    - `mod.rs` - Public exports
  - Added dependencies: serde, serde_json, hex
- [x] 对比维度（建议从严到松）：
  - [x] 必选：DB bytes 完全一致（至少覆盖该合约触达的 store）- compare_kv_data() with KvDiff
  - [ ] 必选：receipt（`ProgramResult.ret` 对应字段）一致 - To be added in execution integration
  - [ ] 可选：state_changes digest 一致（复用 `StateChangeCanonicalizer` 的规则）- Optional enhancement

### 1.4 长跑回归：继续用现成 CSV compare（定位为 nightly/大样本）

已有工具：
- `collect_remote_results.sh`
- `scripts/compare_exec_csv.py`

TODO：
- [ ] 把 fixture conformance 定位为 PR 门禁（快）
  - Recommended CI integration:
    - Java: `./gradlew :framework:test --tests "ProposalFixtureGeneratorTest*" -Dconformance.output=conformance/fixtures`
    - Rust: `cargo test --package tron-backend-core conformance_tests`
- [ ] 把 CSV replay/diff 定位为 nightly（慢，但覆盖真实区块）
  - Continue using existing `collect_remote_results.sh` + `scripts/compare_exec_csv.py` for full chain replay

---

## 4. Phase 2：按依赖分组实现（每组都要“fixture 先红后绿”）

下面每个合约都写了“你实现时最需要的信息”：Java oracle、触达 store、动态开关/receipt、以及建议的 sidecar/receipt 扩展点。

### 2.A（最先做，依赖小）：Proposal 16/17/18

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ProposalDeleteActuator.java`

依赖 store：
- `ProposalStore`（dbName：`proposal`，见 `chainbase/src/main/java/org/tron/core/store/ProposalStore.java`）
- `DynamicPropertiesStore`（`LATEST_PROPOSAL_NUM`、`latest_block_header_timestamp`、`NEXT_MAINTENANCE_TIME`、`MAINTENANCE_TIME_INTERVAL`）
- validate 还依赖 `AccountStore`、`WitnessStore`

TODO：
- [x] Rust：实现 `PROPOSAL_*` handler（validate + execute）
  - **DONE**: Implemented `execute_proposal_create_contract`, `execute_proposal_approve_contract`, `execute_proposal_delete_contract` in `rust-backend/crates/core/src/service/mod.rs:1765-2240`
  - Create: Validates witness status, parses parameters from protobuf, assigns new proposal ID, calculates expiration, creates Proposal proto, persists to ProposalStore, updates LATEST_PROPOSAL_NUM
  - Approve: Validates witness status, parses proposal_id and is_add_approval, adds/removes approval from proposal.approvals, persists updated proposal
  - Delete: Validates owner is proposer, sets state to CANCELED (3), persists updated proposal
- [x] Rust：新增/对齐 ProposalStore adapter（key=`ByteArray.fromLong(id)`；value=Proposal proto bytes）
  - **DONE**: Added `get_proposal()`, `put_proposal()`, `has_proposal()` methods to `EngineBackedEvmStateStore` in `rust-backend/crates/execution/src/storage_adapter/engine.rs:1781-1858`
  - Uses 8-byte big-endian key format via `key_helpers::proposal_key()` matching Java's `ProposalCapsule.createDbKey()`
  - Proposal proto decoding/encoding via prost
- [x] Rust：解决 `proposal_expire_time` 的来源（Java 用 `CommonParameter.getProposalExpireTime()`）：决定是硬编码、读 config、还是从 Java request 透传
  - **DONE**: Added `proposal_expire_time_ms` config field to `RemoteExecutionConfig` in `rust-backend/crates/common/src/config.rs`
  - Default: 259200000 (3 days in milliseconds) matching Java's default
  - Configurable via `config.toml` or default values
- [x] Rust：添加动态属性访问器 `LATEST_PROPOSAL_NUM`, `NEXT_MAINTENANCE_TIME`, `MAINTENANCE_TIME_INTERVAL`
  - **DONE**: Added `get_latest_proposal_num()`, `set_latest_proposal_num()`, `get_next_maintenance_time()`, `get_maintenance_time_interval()` in `engine.rs:1860-1947`
- [x] Rust：添加 config flags for gradual rollout
  - **DONE**: Added `proposal_create_enabled`, `proposal_approve_enabled`, `proposal_delete_enabled` flags to `RemoteExecutionConfig`
  - Default: false for safe rollout, gated in dispatch switch in `service/mod.rs:300-321`
- [ ] Proto/sidecar：为 Proposal 写入引入一种返回通道：
  - 选择：Rust 直接落库（不返回 sidecar），因为 Proposal 操作简单且不需要 Java 二次处理
  - Java apply 不需要实现，因为 Rust 在执行时直接持久化到 ProposalStore
- [x] Java：`RemoteExecutionSPI` 增加 16/17/18 映射（建议 `data = full contract bytes`）
  - **DONE**: Added `ProposalCreateContract`, `ProposalApproveContract`, `ProposalDeleteContract` cases in `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:462-501`
  - Each case extracts full proto bytes and sends as `data` field
- [ ] Java：`RuntimeSpiImpl` 增加 `applyProposalChanges`（带 JVM toggle：`-Dremote.exec.apply.proposal=false`）
  - **NOT NEEDED**: Rust persists directly to ProposalStore, no sidecar/apply needed
- [ ] Fixture：create/approve/delete 的 happy + expired + canceled + repeat approve/unapprove

### 2.B（检验 Account codec 的组）：SetAccountId 19 + AccountPermissionUpdate 46

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java`

依赖 store：
- `AccountStore`（需要完整 Account proto 读写：`account_id`、`owner_permission`、`witness_permission`、`active_permissions`）
- `AccountIdIndexStore`（dbName：`accountid-index`，key=lower(accountId bytes)）
- `DynamicPropertiesStore`（allowMultiSign / totalSignNum / availableContractType / updateAccountPermissionFee / supportBlackHoleOptimization）

TODO：
- [ ] Rust：实现 SetAccountId（同时写 AccountStore + AccountIdIndexStore）
- [ ] Rust：实现 AccountPermissionUpdate（权限校验 + 扣费 + burn/blackhole 逻辑 + 写回权限字段）
- [ ] Rust：补齐 dynamic properties accessor（`ALLOW_MULTI_SIGN`、`TOTAL_SIGN_NUM`、`AVAILABLE_CONTRACT_TYPE`、`UPDATE_ACCOUNT_PERMISSION_FEE`、`ALLOW_BLACKHOLE_OPTIMIZATION`）
- [ ] Proto/receipt：AccountPermissionUpdate 的 `fee` 体现方式要与 Java 一致（写入 `ProgramResult.ret.fee`）
- [ ] Java：RemoteExecutionSPI 增加 19/46 映射
- [ ] Java：若 Rust 选择“只返回 changes 不落库”，则 Java 需要 apply 对应 store（AccountIdIndex/Account permissions）
- [ ] Fixture：
  - [ ] SetAccountId：重复设置 / id 冲突 / owner 不存在
  - [ ] PermissionUpdate：multi-sign 未开启 / 权限 keys 重复 / operations 非法 / fee 不足 / witness permission 条件

### 2.C（合约元数据）：UpdateSetting 33 / UpdateEnergyLimit 45 / ClearABI 48

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/UpdateSettingContractActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UpdateEnergyLimitContractActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ClearABIContractActuator.java`

依赖 store：
- `ContractStore`（dbName：`contract`，value=SmartContract bytes）
- `AbiStore`（dbName：`abi`，value=ABI bytes/AbiCapsule）
- gate：`DynamicPropertiesStore.getAllowTvmConstantinople()`、`ReceiptCapsule.checkForEnergyLimit(...)`

TODO：
- [ ] Rust：实现 33/45：读取 SmartContract → 修改字段 → 写回（并处理 Repository LRU cache 的等价行为：通常可忽略/由 Java cache 层解决）
- [ ] Rust：实现 48：写 AbiStore 为默认 ABI（并满足 owner 校验）
- [ ] Rust：补齐 ContractStore/AbiStore adapter + proto decode（SmartContract proto 在 `protocol/src/main/protos/core/contract/smart_contract.proto`）
- [ ] Java：RemoteExecutionSPI 增加 33/45/48 映射
- [ ] Proto/sidecar：需要能表达 ContractStore/AbiStore 的写入（推荐 DbKvChange）
- [ ] Fixture：owner 不是 originAddress；contract 不存在；Constantinople 未开启；originEnergyLimit<=0

### 2.C2（小而关键）：UpdateBrokerage 49

Java oracle：`actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java`

依赖 store：
- `DelegationStore`（dbName：`delegation`；key 格式见 `chainbase/src/main/java/org/tron/core/store/DelegationStore.java`）
- validate 依赖：`DynamicPropertiesStore.allowChangeDelegation()`、`WitnessStore`、`AccountStore`

TODO：
- [ ] Rust：实现 49（validate：开关/地址/范围/必须是 witness；execute：`DelegationStore.setBrokerage(-1, owner, brokerage)`）
  - Rust 侧 key 生成可复用：`rust-backend/crates/execution/src/delegation/keys.rs`（`delegation_brokerage_key(-1, owner)`）
- [ ] Proto/sidecar：表达对 `delegation` DB 的写入（推荐 DbKvChange）
- [ ] Java：RemoteExecutionSPI 增加 49 映射（建议 `data = full UpdateBrokerageContract bytes`）
- [ ] Fixture：brokerage 边界（0/100/负数/超 100）、owner 非 witness、allowChangeDelegation=false

### 2.D（资源/冻结/委托）：WithdrawExpireUnfreeze 56 / DelegateResource 57 / UnDelegateResource 58 / CancelAllUnfreezeV2 59

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java`

依赖 store（高耦合）：
- `AccountStore`（`unfrozenV2`、`frozenV2`、delegated/acquired balances、netUsage/energyUsage/timestamps/window）
- `DelegatedResourceStore`（dbName：`DelegatedResource`，key 前缀规则见 `DelegatedResourceCapsule`）
- `DelegatedResourceAccountIndexStore`（dbName：`DelegatedResourceAccountIndex`，V2 前缀 `0x03/0x04`）
- `DynamicPropertiesStore`（supportDR / supportUnfreezeDelay / allowCancelAllUnfreezeV2 / total weights & limits / latest timestamp 等）

receipt：
- 56：`withdraw_expire_amount`
- 59：`withdraw_expire_amount` + `cancel_unfreezeV2_amount` map

TODO（这组的前置条件很多）：
- [ ] Phase 0 的 Account codec + AEXT/resource 字段通路必须先完成（否则无法正确更新 usage/window）
- [ ] Rust：实现 WithdrawExpireUnfreeze（按时间过滤 unfrozenV2，balance += sum(expired)，清理列表，返回 withdraw_expire_amount）
- [ ] Rust：实现 CancelAllUnfreezeV2（遍历 unfrozenV2：未到期→回冻并更新 total weights；到期→加到 withdraw_expire_amount；最后清空 unfrozenV2；返回 cancel map）
- [ ] Rust：实现 DelegateResource/UnDelegateResource（需要严格对齐 Java 的锁期、解锁、usage 迁移、index store 更新逻辑）
- [ ] Proto/receipt：落实 Phase 0 的 receipt 回传方案（否则 56/59 的 TransactionInfo 一定错）
- [ ] Java：RemoteExecutionSPI 增加 56/57/58/59 映射；RuntimeSpiImpl 增加对应 apply（或使用 DbKvChange）
- [ ] Fixture：
  - [ ] 56：无可提取/恰好到期/溢出边界
  - [ ] 59：三资源类型混合；部分到期部分未到期；验证 total weights 变化
  - [ ] 57/58：lock/非 lock；lockPeriod 边界；receiver 为合约地址；重复 delegate/unDelegate 顺序依赖

### 2.E（TRC-10 扩展）：ParticipateAssetIssue 9 / UnfreezeAsset 14 / UpdateAsset 15（+ 可能的 VoteAsset 3）

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UnfreezeAssetActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java`

依赖 store：
- `AccountStore`（asset map、frozen_supply、asset_issued_name/id）
- `AssetIssueStore` + `AssetIssueV2Store`（allowSameTokenName 分支）
- `DynamicPropertiesStore`（allowSameTokenName、latest timestamp、oneDayNetLimit 等）

TODO：
- [ ] 先决定“TRC-10 的远端落库语义”：目前 TRC-10 apply 是 delta，且 RemoteExecutionSPI 还 gate（`remote.exec.trc10.enabled=false` 默认）
- [ ] Proto：扩展 `Trc10Change` oneof：
  - [ ] 增加 `Trc10Participated`（参与发行：owner 扣 TRX、issuer 加 TRX、asset 在两边转移）
  - [ ] 增加 `Trc10UnfreezeSupply`（解除冻结供应）
  - [ ] 增加 `Trc10AssetUpdated`（更新 url/description/limits）
- [ ] 或者：统一用 DbKvChange（Account/AssetIssue* 写入都能表达，避免 oneof 膨胀）
- [ ] VoteAssetContract：先确认是否已在本 repo 里“事实上废弃”（当前没有 actuator）；若需要支持：
  - [ ] 用 `git log -S "VoteAsset"` 找历史实现/参考旧版本
  - [ ] 没有 oracle 时：写 spec-based fixture（来自 TRON 协议文档）+ 与节点行为对照

### 2.F（Exchange 41-44）：复杂但可做（先补 receipt 通道再动）

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeInjectActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeWithdrawActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/ExchangeTransactionActuator.java`

依赖 store：
- `AccountStore`（TRX + asset map）
- `ExchangeStore` / `ExchangeV2Store`
- `AssetIssueStore`（allowSameTokenName=0 时用于 name→real id）
- `DynamicPropertiesStore`（latestExchangeNum、exchangeBalanceLimit、allowStrictMath、exchangeCreateFee、supportBlackHoleOptimization）

receipt：
- 41：`exchange_id`
- 42：`exchange_inject_another_amount`
- 43：`exchange_withdraw_another_amount`
- 44：`exchange_received_amount`

TODO：
- [ ] Phase 0 receipt 回传必须先做（否则 TransactionInfo 一定错）
- [ ] Rust：实现 ExchangeCapsule.transaction 的等价逻辑（严格对齐 strictMath 分支）
- [ ] Rust：实现 create/inject/withdraw/transaction + fee/burn/blackhole 语义
- [ ] Proto/sidecar：表达 ExchangeStore/ExchangeV2Store/DynamicProperties 的写入（推荐 DbKvChange）
- [ ] Java：RemoteExecutionSPI 增加 41-44 映射；RuntimeSpiImpl apply（或 DbKvChange）
- [ ] Fixture：覆盖 allowSameTokenName=0/1；trx/token 组合；balanceLimit；expected 小于返回量的 validate-fail

### 2.G（Market 52-53）：工作量大（建议独立里程碑）

Java oracle：
- `actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java`
- `actuator/src/main/java/org/tron/core/actuator/MarketCancelOrderActuator.java`

依赖 store（非常多）：
- `MarketAccountStore` / `MarketOrderStore` / `MarketPairToPriceStore` / `MarketPairPriceToOrderStore`
- `AccountStore` + `AssetIssueStore/V2` + `DynamicPropertiesStore`

receipt：
- 52：`orderId` + `orderDetails[]`
- 53：无额外字段，但会影响 order book 状态

TODO：
- [ ] 先把 receipt bytes 通道打通（orderDetails 很难逐字段扩）
- [ ] 以 DbKvChange + receipt bytes 的方式推进（否则 sidecar 会爆炸）
- [ ] 分阶段：
  - [ ] 先做 cancel（相对 sell 简单）
  - [ ] 再做 sell 的“无撮合/少撮合”路径
  - [ ] 最后做撮合循环 + MAX_MATCH_NUM 边界 + 价格队列清理

### 2.H（Shield 51）：最后做（高风险、高依赖）

Java oracle：`actuator/src/main/java/org/tron/core/actuator/ShieldedTransferActuator.java`

现实约束：Java 依赖 native zk lib（`JLibrustzcash`），Rust 端要做到同样验证需要引入对应实现与参数；且 store（Nullifier/Merkle/ZKProof/totalShieldedPoolValue）复杂。

TODO（建议）：
- [ ] 先决定产品策略：是否长期保持 Java path（RemoteExecutionSPI 不映射/强制 fallback）
- [ ] 若必须实现：拆成独立 roadmap（proof 校验、merkle、nullifier、receipt fee、pool value）

### 2.I（VM/查询类）：CreateSmartContract 30 / TriggerSmartContract 31 / GetContract 32 / CustomContract 20

现状提示：
- Java 已把 30/31 映射成 `tx_kind=VM`（`RemoteExecutionSPI.buildExecuteTransactionRequest`），Rust 侧走 `ExecutionModule.execute_transaction_with_storage(...)`。
- 但你表里标记为 ❌ 的原因通常不是“没有入口”，而是 **TRON-TVМ 语义/落库/回执不完整**（尤其是 create 的合约元数据落库、能量/回执字段、以及 CreateSmartContract 的 toAddress 语义）。

TODO（分三层推进）：
- [ ] L1：先把 **CreateSmartContract 的 toAddress 语义**修正并加测试（见 Phase 0.5），确保“创建”不会被误当“call 0 地址”。
- [ ] L2：补齐“合约创建后必须落库/可查询”的状态面：
  - [ ] contract/code/abi/contract-state 的 key/value 规则与 Java 对齐（参见 `RepositoryImpl.commit*Cache`，store 在 `chainbase/.../store/*Store.java`）
  - [ ] 远端回传 receipt/ProgramResult.ret（至少保证 `TransactionInfo.contractAddress/contractResult/fee` 正确）
- [ ] L3：做 VM parity fixtures（最小合约部署 + 调用）：
  - [ ] deploy：bytecode 仅写存储/返回常量；对比：codeStore/contractStateStore/contractStore 的最终 bytes
  - [ ] trigger：调用后 storage slot 变化 + return_data
  - [ ] edge：revert/out-of-energy/invalid opcode（对比 `contractRet` 与 runtimeError）
- [ ] GetContract（32）与 CustomContract（20）：先确认“是否真的作为交易执行路径存在”
  - [ ] 在本 repo 搜索是否有 actuator/执行逻辑；若没有则明确策略：保持 Java fallback / 标记不支持 / 走单独 RPC 而非 ExecuteTransaction
  - [ ] 若必须支持 GetContract：更可能是 **查询**（应落在 storage service 或新增 gRPC API），而不是执行交易

---

## 5. Phase 3：灰度、回归、CI 门禁

TODO：
- [ ] Rust：为每个新 contract type 增加 `execution.remote.<contract>_enabled`（默认 false）并在 dispatch 里 gate
- [ ] Java：RemoteExecutionSPI 增加 JVM property gate（与 Rust 配合，确保可随时回滚到 embedded）
- [ ] PR 门禁：
  - [ ] 跑 fixture conformance（覆盖所有新增 contract 的 happy/validate-fail/edge）
  - [ ] 跑 `./gradlew :framework:test`（或按 contract 过滤）
  - [ ] 跑 `cargo test`（只跑新增 fixture runner + unit）
- [ ] Nightly：
  - [ ] `collect_remote_results.sh` 回放 + `scripts/compare_exec_csv.py` diff
  - [ ] 若要更强一致性：把 Domain/State digest 作为 alert 指标（已有 `StateChangeCanonicalizer` / `DomainCanonicalizer`）

---

## 6. 未实现合约清单（按优先级建议）

优先级建议（从快到慢）：
1) Proposal 16/17/18  
2) SetAccountId 19、AccountPermissionUpdate 46  
3) UpdateSetting 33、UpdateEnergyLimit 45、ClearABI 48、UpdateBrokerage 49  
4) WithdrawExpireUnfreeze 56、CancelAllUnfreezeV2 59  
5) Delegate/UnDelegate 57/58  
6) TRC-10 扩展 9/14/15（以及确认 VoteAsset 3 是否需要）  
7) Exchange 41-44  
8) Market 52-53  
9) Shield 51

补充（单独里程碑）：
- 30/31（VM：Create/TriggerSmartContract）：走 TVM parity 路线，优先把 create 语义与落库/receipt 打通，再做系统性 conformance
- 32/20（GetContract/CustomContract）：先确认是否属于“交易执行”范畴；很可能应保持 Java fallback 或改为单独查询 RPC

---

## 7. 参考资料/已有 planning 文档（可复用）

- AEXT（Account resource usage tail）方案：`planning/rust_account_ser.todo.md`
- CSV 对比体系：`planning/csv_comparator.todo.md`
- 其它历史 planning：`planning/fast_do.planning.md`、`planning/more_contract_type.todo.md`
