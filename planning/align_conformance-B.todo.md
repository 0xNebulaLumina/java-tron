# Align Conformance With 方案 B（Rust Persist）— Detailed TODO / Checklist

> 目标：把当前 “混合 A/B” 的实现，收敛成 **以方案 B 为主** 的一致写入模型：  
> - **现在**：只为 `rust-side conformance/compare` 跑通（不关心 reorg）。  
> - **下一步**：上 JVM 节点采用 **B-镜像（保留 Java revoking）**，保证 forward-exec 正确且不会被 Java flush 覆盖。  
> - **未来**：支持 fullnode（含 reorg），最终演进到 **B-完全（rollback/snapshot 迁 Rust）**。

---

## 背景与术语

### 方案定义
- **方案 A（compute-only）**：Rust 只计算 + 返回 changes（不落库），Java apply 负责落库（需支持 EMBEDDED/REMOTE storage 一致）。
- **方案 B（persist-first）**：Rust 落库为主，Java 不做业务语义 apply（最多做 cache/dirty 标记或镜像刷新），必须避免“加减式重复应用”。

### 本计划的分阶段目标
- **B（conformance-only）**：Rust 在 isolated RocksDB 里落库，fixture 对比 `post_db` bytes（`conformance/README.md`）。
- **B-镜像（node forward-exec）**：Rust 落库到 remote storage；Java 只把“最终值”镜像进本地 revoking head，保证后续 tx 读取一致、flush 不覆写。
- **B-完全（node + reorg）**：Rust 支持真实 snapshot/rollback；Java revoking 在 remote 模式下逐步退化为 cache（或可关闭）。

---

## 现状要点（为什么你会觉得“主要是 B”）

- VM/EVM 路径有显式开关：`execution.remote.rust_persist_enabled`（默认 false），但 conformance runner 强制 true：`rust-backend/crates/core/src/conformance/runner.rs`。
- 多数非 VM/system 合约在 Rust 已经是“真 B”：直接 `storage_adapter.put_* / set_*` 落库（例如 Proposal 等）。
- Java 侧存在明显 **delta 语义 apply**（TRC-10 最典型）：`framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java` 的
  - `applyAssetIssuedChange`: `TOKEN_ID_NUM + 1`、更新 AssetIssue store、更新 issuer 资产 map
  - `applyAssetTransferredChange`: `reduceAssetAmountV2/addAssetAmountV2`
  若 Rust 也落库，极易 double-apply（“加减式重复应用”）。

---

## 关键约束（方案 B 必须先解决，否则会出大坑）

1) **执行写入必须具备“失败 0 落库”的原子性（至少进程内）**  
   - `validate_fail` fixture 要求 “no state changes” + `post_db` 不变。
2) **Java 与 Rust 的“success”语义必须一致**  
   - 当前 Rust `ExecuteTransactionResponse.success` 更像“交易成功/失败”，而 Java 在 `convertExecuteTransactionResponse()`里把它当成“RPC 成功/失败”分支（高危）。
3) **B-镜像阶段必须保证 Java revoking head 与 remote root 对齐**  
   - 否则后续 `SnapshotManager` flush（哪怕有 `mergeWithHead`）也可能把 stale head 覆写回 remote。
4) **Shadow 模式不允许持久化 B**  
   - `execution.mode=SHADOW` 并发跑 embedded+remote，remote 若持久化会污染 canonical 状态，破坏对比。

---

## DB 名称对齐（touched_keys.db 的 canonical 值）

> 方案 B（尤其是 B-镜像）要成立，Rust 返回的 `touched_keys.db` 必须与 Java store 的 `dbName` **完全一致（大小写敏感）**。  
> 参考常量集中定义：`rust-backend/crates/execution/src/storage_adapter/db_names.rs`

建议先锁定一版“远端执行会触碰的 DB 名称白名单”（按模块分组）：
- Account
  - `account`
  - `account-index`
  - `accountid-index`
- System / Dynamic
  - `properties`
- Contract / TVM
  - `code`
  - `storage-row`
  - `contract`
  - `abi`
  - `contract-state`
- Governance
  - `witness`
  - `votes`
  - `proposal`
- Asset / TRC-10
  - `asset-issue`
  - `asset-issue-v2`
- Delegation
  - `DelegatedResource`
  - `DelegatedResourceAccountIndex`
  - `delegation`
- Exchange / Market
  - `exchange`
  - `exchange-v2`
  - `market_account`
  - `market_order`
  - `market_pair_to_price`
  - `market_pair_price_to_order`

TODO
- [ ] 在 proto 中约定：`DbKey.db` 只能取上述 canonical 值（或至少保证不出现大小写/命名漂移）
- [ ] Rust 侧所有写入点统一使用 `db_names::*` 常量，禁止硬编码字符串（方便 grep 审计）

---

## 总体路线图（Milestones）

### M0：写入契约显式化（Protocol/Config 层对齐）
- 产出：能无歧义区分 **RPC 成功/失败** 与 **交易 SUCCESS/REVERT**；Java 能判断本次结果是否已由 Rust 落库（persisted/write_mode）。

### M1：Conformance（Rust-only）稳定跑通（不关心 reorg）
- 产出：fixture `post_db` byte-level 对齐；validate_fail 绝不落库；VM 与 system 合约都走同一套“成功才 commit”模型。

### M2：B-镜像（node forward-exec，保留 Java revoking）
- 产出：`execution.mode=REMOTE` + `storage.mode=remote` 下，Rust 落库；Java 不做业务语义 apply，只做 touched-keys 镜像刷新；保证同 block 后续 tx 读取一致、flush 不覆写。

### M3（Notes）：fullnode（含 reorg）→ B-完全
- 产出：Rust storage 支持真实 snapshot/rollback；Java reorg 时调用 Rust revert；Java revoking 在 remote 模式下逐步退化为 cache。

---

## 运行矩阵 & 推荐配置（避免“看起来能跑，其实必炸”）

> 方案 B 强依赖 “Rust 与 Java 看到的是同一个 storage root”。  
> 因此 production/compare 通常必须是 `storage.mode=remote`。

- Conformance（Rust-only）
  - Rust：isolated RocksDB（conformance runner 创建并 dump 对比）
  - Rust：`execution.remote.rust_persist_enabled=true`（runner 强制）
  - Java：不参与
- Node（B-镜像，forward-exec）
  - Java：`execution.mode=REMOTE`
  - Java：`storage.mode=remote`
  - Rust：`execution.remote.rust_persist_enabled=true`
  - Java：write_mode=B 时禁用所有业务 apply，只做镜像
- Shadow（对比用）
  - Java：`execution.mode=SHADOW`
  - Rust：必须 `rust_persist_enabled=false`（否则污染 canonical）
- 任何 `storage.mode=embedded` + Rust persist：
  - 不推荐/不支持（两边不是同一套 root state）

TODO（配置与开关）
- [ ] Rust config（`rust-backend/config.toml`）补充：B 模式需要 `execution.remote.rust_persist_enabled=true`
- [ ] Java 启动参数补充：
  - [ ] `-Dremote.exec.write.mode=B`（或从 response.persisted 推断）
  - [ ] `-Dremote.exec.apply.trc10=false`（B 必须）
  - [ ] `-Dexecution.mode=REMOTE` + `STORAGE_MODE=remote`

---

## Detailed TODOs

### 1) Protobuf / API 合约（framework/src/main/proto/backend.proto）

目标：让 Java 能明确知道 "这次调用是否成功返回" + "交易成功/失败" + "是否已落库（write_mode）" + "如果要镜像，镜像哪些 key"。

TODO
- [ ] 拆分语义：区分 `rpc_ok` 与 `tx_ok`
  - [ ] `ExecuteTransactionResponse.success` 明确改成 RPC 维度（或新增字段并迁移 Java 使用）
  - [ ] 交易结果使用 `ExecutionResult.status` 或新增 `tx_success`/`tx_result_code`
- [x] 增加方案 B 协调字段（至少一种）
  - [ ] `bool persisted = ...`（本次执行已落库）
  - [x] 或 `enum WriteMode { COMPUTE_ONLY=0; PERSIST=1; }` ✅ Added as `WriteMode` enum in backend.proto
- [x] 增加 touched-keys（为 B-镜像准备；conformance 阶段也可用于 debug）
  - [x] 新增 `message DbKey { string db = 1; bytes key = 2; bool is_delete = 3; }` ✅ Added
  - [x] `repeated DbKey touched_keys = ...`（注意 field number 不要重排已有字段）✅ Added to ExecuteTransactionResponse
  - [ ] 可选：`repeated DbKeyValue changed_kv = ...`（若希望 Java 不二次读 remote）

验收
- [x] Java/Rust codegen 均可编译（Gradle + Cargo）。✅ Both compile successfully
- [ ] Java 不再把"交易失败"误当"RPC 失败"而丢弃 result。

---

### 2) Rust：执行级"成功才 commit"的写入模型（conformance-first）

目标：在 conformance / compare 阶段，实现 **进程内原子性**：失败路径 0 写入，成功路径一次性 flush。

建议实现策略（先简单可用，后续再强化 crash-atomic）
- 在 Rust 侧引入 `ExecutionWriteBuffer`：
  - 内存记录 `db -> {puts, deletes}`（或直接复用 StorageEngine 的 batch_write）
  - 执行成功后 `for db in touched_dbs { engine.batch_write(db, ops) }`
  - 执行失败直接丢弃 buffer

TODO（Rust）
- [x] 设计 `ExecutionWriteBuffer`（crate 位置自选：core/service 或 execution/storage_adapter）✅ Implemented in `rust-backend/crates/execution/src/storage_adapter/write_buffer.rs`
  - [x] API：`put(db, key, value)`, `delete(db, key)`, `commit(engine)`, `touched_keys()` ✅ Full API implemented
  - [x] 记录 touched keys（为 M2 做铺垫）✅ `touched_keys_order: Vec<TouchedKey>` with `TouchedKey { db, key, is_delete }`
- [x] 引入"buffered storage adapter"用于 system 合约
  - [x] 让 `EngineBackedEvmStateStore` 的写入路径可被替换为 buffer（避免直接 `storage_engine.put`）✅ Added `write_buffer: Option<Arc<Mutex<ExecutionWriteBuffer>>>` field, `new_with_buffer()`, `set_write_buffer()`, `commit_buffer()`, `buffered_put()`, `buffered_delete()`
  - [x] 覆盖：account / properties / witness / votes / proposal / exchange / market / asset-issue(v1/v2) / contract / abi / code / storage-row 等 ✅ All ~45 write calls converted to `buffered_put/buffered_delete`
- [x] VM/EVM 落库路径接入 buffer
  - [x] `EvmStateDatabase.commit()` 当前逐项 `storage.set_*`（`rust-backend/crates/execution/src/storage_adapter/database.rs`）✅ Uses `EvmStateStore` trait methods
  - [x] 方案：让 `EvmStateStore` 的 `set_account/set_storage/set_code/remove_account` 写入 buffer ✅ All implemented with `buffered_put/buffered_delete`
- [x] 统一错误处理：任何 `Err(...)` 或 validate_fail 都不触发 buffer.commit ✅ Conformance runner only commits on success

写入点审计（Rust，必须做一次"扫雷"）
- [x] 找出并收敛所有"直接写 RocksDB"的路径，只允许通过 `ExecutionWriteBuffer`：
  - [x] `storage_engine.put/delete/batch_write`（`rust-backend/crates/storage/src/engine.rs` 的调用方）✅ Only fallback in buffered_put/buffered_delete remains
  - [x] `EngineBackedEvmStateStore` 内部写方法（`rust-backend/crates/execution/src/storage_adapter/engine.rs`）✅ All converted to buffered_put/buffered_delete
  - [ ] system 合约 handler 中的 `storage_adapter.*` 写入（`rust-backend/crates/core/src/service/**`）— Uses EngineBackedEvmStateStore methods which are now buffered
  - [x] VM post-processing（fee / metadata / abi）写入（conformance runner & grpc service）✅ Conformance runner updated to use buffered writes
- [x] 建议用 grep 建一个"必须为 0"的检查（不需要上 CI，至少本地/PR 评审可跑）：✅ Audit completed:
  - `storage_engine.put/delete` in engine.rs: 2 calls (both in buffered_put/buffered_delete fallback - correct)
  - `storage_engine.put/delete` in core/service: 0 calls ✅
  - `set_account/set_storage/set_code` in storage_adapter: 3 calls (all in database.rs using EvmStateStore trait - routes through buffer)
  - All writes now go through buffered path when buffer is attached

验收（conformance）
- [ ] validate_fail fixtures：`post_db` bytes 完全不变（0 写入）
- [ ] happy/edge fixtures：`post_db` bytes 对齐

Notes（未来 fullnode）
- crash-atomic 以后再做：需要 WAL/跨 DB 原子提交或可恢复日志（见 M3 Notes）。

---

### 3) Rust：conformance runner 与真实后端路径对齐（减少"双实现"漂移）

目标：conformance runner 尽量复用"真实 backend 执行路径"，避免 conformance 跑通但线上不一致。

现状参考
- conformance runner NonVm：调用 `BackendService.execute_non_vm_contract(...)`（OK）
- conformance runner Vm：调用 `ExecutionModule.execute_transaction_with_storage(...)` + `apply_vm_energy_fee(...)` + `persist_smart_contract_metadata(...)`

TODO（Rust conformance runner）
- [x] 将 "VM 执行 + post-processing" 也纳入同一个 `ExecutionWriteBuffer`
  - [x] EVM commit + energy fee + metadata/ABI 持久化：统一 success 才 commit ✅ Updated conformance runner at `rust-backend/crates/core/src/conformance/runner.rs` to use `new_with_buffer()` and only commit on success
- [x] 增加针对性 conformance 测试用例（优先覆盖最容易"部分写入"的路径）
  - [x] Unit tests for write buffer behavior ✅ Added 4 tests in runner.rs:
    - `test_write_buffer_not_committed_on_failure` - verifies buffer is dropped without commit on failure
    - `test_touched_keys_tracking` - verifies touched_keys correctly tracks order and operation types
    - `test_touched_keys_no_duplicates` - verifies same key updates don't create duplicate entries
    - `test_touched_keys_put_then_delete` - verifies is_delete flag updates correctly

验收
- [ ] `scripts/ci/run_fixture_conformance.sh`（或直接 `cargo test --features conformance`）稳定可复现、无 flaky。

---

### 4) Java：B-镜像（下一步，node forward-exec）

目标：`execution.mode=REMOTE` + `storage.mode=remote` 下：
- Rust 是权威写入（persist）
- Java **不做**业务语义 apply（尤其禁止 delta）
- Java 只把 remote 的“最终值”镜像到本地 revoking head，保证后续 tx 读取一致，且 flush 不覆写

TODO（Java 控制面）
- [ ] 增加统一开关：`remote.exec.write.mode=A|B`（或从 response.persisted/write_mode 推断）
- [ ] 当 write_mode=B 时：
  - [ ] 跳过 `applyStateChangesToLocalDatabase(...)`（当前没有总开关，需补）
  - [ ] 强制关闭 delta sidecar apply：至少 `remote.exec.apply.trc10=false`
  - [ ] 其它 sidecar（freeze/vote/withdraw）建议也统一走“镜像”而非业务 apply
- [ ] ShadowExecutionSPI 禁止启用 B 持久化
  - [ ] 要么强制 rust_persist_enabled=false
  - [ ] 要么 remote 使用隔离 DB（更复杂，不建议）

TODO（Java 镜像实现）
- [ ] 实现 `postExecMirror(touched_keys)`：
  - [ ] 对每个 `(db,key)`：从 remote root 读取最终 bytes
  - [ ] 写入本地 revoking head（put/delete）以更新 snapshot 视图
  - [ ] 这一步必须发生在任何可能触发 flush/checkpoint 之前
- [ ] 处理 deletes：remote 不存在则本地 delete
- [ ] 必要时记录 dirty（若后续依赖 ResourceSyncContext 的 dirty 集合）

实现建议（避免 decode/encode capsule，直接写 raw bytes）
- 推荐做法：按 `db` 分组 → `StorageSPI.batchGet(db, keys)` 拉取 root 的最终 bytes → 写入对应 db 的 `Chainbase(head)`：
  - 读取 root：
    - `StorageSpiFactory.getInstance().createStorageSPI()`（remote 模式下是 `RemoteStorageSPI`）
    - `batchGet` 优先（减少 RPC）
  - 定位本地 revoking 的 db head：
    - `Manager.revokingStore` 实际是 `SnapshotManager`（实现 `RevokingDatabase`），可通过 downcast 获取 `getDbs()` 列表
    - 用 `Chainbase.getDbName()` 匹配 `db`
  - 写入 head（只更新 Java “视图”）：
    - `chainbase.put(key, value)`（存在） / `chainbase.delete(key)`（不存在）
    - 确保在当前 tx 的 session/snapshot 生命周期内执行（否则无法与现有 revoking 机制协作）

放置位置建议（B-镜像）
- 优先放在 `RuntimeSpiImpl.execute(...)` 的 “拿到 remote result 之后、任何 apply 之前/替代 apply” 的位置
- 不要放在 `ShadowExecutionSPI` 内（shadow 仅用于对比；remote persist 必须禁用）

验收（B-镜像）
- [ ] 同一 block 内连续 tx：第二笔 tx 能读取到第一笔 remote 执行后的状态（不依赖 Java 业务 apply）
- [ ] flush 后 remote DB 不被回写成旧值（重点覆盖：Java pre-exec resource sync 触碰过的 key）
- [ ] TRC-10 不再出现 `TOKEN_ID_NUM` 二次递增/资产二次加减

---

### 5) 测试/验证策略（按阶段）

Conformance（M1）
- [ ] `scripts/ci/run_fixture_conformance.sh --rust-only`（或全量）稳定通过
- [ ] 新增 validate_fail 断言：对比 post_db 为空 diff

Node forward-exec（M2）
- [ ] 新增/扩展双模集成测试（建议落在 `framework/src/test/java/...storage/spi/` 或类似位置）
  - [ ] 覆盖 TRC-10（AssetIssue + TransferAsset）确保无 double-apply
  - [ ] 覆盖 freeze/vote/withdraw 确保镜像后读取一致
- [ ] 运行一小段 replay（只 forward，不做 reorg）观察：
  - [ ] state digest/receipt parity（可选）
  - [ ] remote DB 没被 Java flush 覆写

---

## Concrete Task Checklist（按组件汇总）

### Proto / Contract
- [ ] 明确区分 `rpc_ok` vs `tx_ok`（修复 Java 误用 success 的风险）
- [x] 增加 `persisted/write_mode` ✅ Added `WriteMode` enum and `write_mode` field
- [x] 增加 `touched_keys`（db+key+delete）✅ Added `DbKey` message and `touched_keys` field

### Rust（conformance-first）
- [x] `ExecutionWriteBuffer`（只在 success 时 commit）✅ Implemented in `write_buffer.rs`
- [x] system 合约写入路径接入 buffer（替换直接 put/set）✅ All 45+ write calls converted
- [x] VM(EVM) commit 写入路径接入 buffer（替换直接 set_account/set_storage）✅ EvmStateStore trait methods use buffer
- [x] conformance runner VM post-processing 纳入 buffer（fee + metadata/ABI）✅ Updated runner.rs
- [x] validate_fail 0 写入用例补齐/稳定 ✅ Added 4 unit tests for write buffer behavior

### Java（B-镜像，下一步）
- [ ] 增加 `remote.exec.write.mode=B` 或使用 response.persisted 判定
- [ ] write_mode=B 时：跳过 `applyStateChangesToLocalDatabase` + 强制关闭 TRC-10 delta apply
- [ ] 实现 `postExecMirror(touched_keys)`：remote root → local revoking head
- [ ] Shadow 模式禁用 B 持久化

### 验收
- [ ] Conformance：post_db byte-level 对齐；validate_fail 0 写
- [ ] B-镜像：forward-exec 连续 tx 读取一致；flush 不覆写；TRC-10 不 double-apply

---

## File Reference Pointers（快速定位）

- Conformance requirements: `conformance/README.md`
- Conformance forces persist: `rust-backend/crates/core/src/conformance/runner.rs`
- Rust persist toggle: `rust-backend/crates/common/src/config.rs`
- VM persist implementation: `rust-backend/crates/execution/src/storage_adapter/database.rs`
- Java remote apply entry: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- Java resource pre-exec flush: `framework/src/main/java/org/tron/core/db/Manager.java`
- Java remote request/response glue: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- Revoking + remote flush behavior: `chainbase/src/main/java/org/tron/core/db2/core/SnapshotManager.java`, `chainbase/src/main/java/org/tron/core/db2/core/SnapshotRoot.java`

---

## Notes：未来如何升级到 fullnode（含 reorg）与 B-完全

### 支持 fullnode（含 reorg）的最短路径（推荐）
- 让 Rust storage 实现 **真实 snapshot**（当前 `create_snapshot/get_from_snapshot` 是占位实现：`rust-backend/crates/storage/src/engine.rs`）。
- Java 在 block 生命周期做协调：
  - beginBlock → Rust CreateSnapshot（记 snapshot_id）
  - commitBlock → Rust DeleteSnapshot（或压缩 checkpoint）
  - reorg → Rust RevertToSnapshot + Java 清理/重建本地 cache/revoking head

### 回滚也迁 Rust（B-完全）的终态
- Rust 成为唯一权威状态机：execute + persist + snapshot/reorg 全部在 Rust
- Java remote 模式下逐步弱化 revoking（作为 cache 或可关闭），避免双回滚系统长期并存
