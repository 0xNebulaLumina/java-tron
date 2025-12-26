• 目标与取舍（你当前的偏好）                       
                                                                                                                                                                                                                  
- 现在：方案 B 先服务于 rust-side conformance/compare，允许“不关心 reorg”，优先让 Rust 侧落库 + fixture post_db byte-level 对齐跑通。                                                                           
- 下一步：在 Java-Tron 节点里落地 B-镜像（保留 Java revoking/snapshot 体系），先跑“前向执行”正确且不被 Java flush 覆盖。                                                                                        
- 以后：演进到 fullnode（含 reorg），最终走 B-完全（rollback/snapshot 也迁 Rust）。                                                                                                                             
                                                                                                                                                                                                                
———                                                                                                    
                                                                                                                                                                                                                
## Phase 1：把“方案 B 的写入契约”变成显式（先把坑堵住）                                                                                                                                                         
                                                                                                       
1. 明确 gRPC 返回语义（必须优先做）                                                                    
                                                                                                                                                                                                                
- 现在 Rust ExecuteTransactionResponse.success 被设置成“交易成功/失败”（rust-backend/crates/core/src/service/grpc/conversion.rs），但 Java RemoteExecutionSPI.convertExecuteTransactionResponse()把它当成“RPC 成
  功/失败”用（framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java）。                                                                                                                   
- 方案 B 下这是高危：可能出现“Rust 已落库，但 Java 因为 success=false 抛弃 result/跳过镜像”。          
- 计划：把 RPC 成功 与 交易 SUCCESS/REVERT 拆开（例如新增 rpc_success/grpc_ok，或者统一用 error_message + result.status 表达交易状态）。                                                                        
                                                                                                       
2. 统一一个 write-mode（避免 Java/Rust 各自猜）                                                                                                                                                                 
                                                                                                                                                                                                                
- Rust 已有：execution.remote.rust_persist_enabled（rust-backend/crates/common/src/config.rs / rust-backend/crates/execution/src/lib.rs / rust-backend/crates/execution/src/storage_adapter/database.rs）。
- 计划：在协议或配置层引入“本次执行是否已持久化”的显式信号（例如 ExecuteTransactionResponse.persisted=true 或 write_mode=B），让 Java 决定：
    - 是否跳过 applyStateChangesToLocalDatabase(...)      
    - 是否跳过 applyFreeze/Trc10/Vote/Withdraw... 
                                                                                                                                                                                                                
3. 执行级原子性：validate_fail / error path 必须 0 落库（conformance 的底线）                                                                                                                                   
                                                                                                                                                                                                                
- 方案 B 的 fixture 要求 post_db bytes 精确一致（conformance/README.md），validate_fail 用例要求“不改任何 DB”。                                                            
- 计划：Rust 侧执行引入“transaction-scoped write batch/transaction”，保证：                                                                                                                                     
    - 成功：一次性 commit                                                                                                                                                                                       
    - 失败/返回 Err：rollback（不写入底库）                                                                                                                                                                     
- 落点：EVM commit（EvmStateDatabase.commit()）+ system 合约写（大量 storage_adapter.put_*）都要写进同一个 batch，而不是直接 put 到 RocksDB。                                                                   

———

## Phase 2：conformance/compare（当前阶段：不关心 reorg）

目标是“Rust 自己把 post_db 写对”，Java 不参与。

1. 确定 conformance 路径就是 B（你现在看到的现象是合理的）

- conformance runner 强制 rust_persist_enabled=true（rust-backend/crates/core/src/conformance/runner.rs），因为它在 isolated RocksDB 上对比 post_db。
- 所以“全改成方案 A”会导致大量 conformance fixture 直接不匹配：Rust 不落库就没 post_db 可比（除非你重写 conformance 规则，让它比 stateChanges/changeset 而不是 post_db bytes）。

2. 把“只发 sidecar、但不落库”的合约/字段逐步迁到 Rust 落库

- 你现在的代码里有明显的 hybrid：
    - TRC-10：Rust 只发 trc10_changes，Java applyTrc10Changes 做了 TOKEN_ID_NUM +1 和 reduce/add（framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java）——这在方案 B 下要么迁 Rust 落库，要么
      Java 必须彻底停掉。
    - Freeze/Vote/Withdraw：Rust 往往落一部分（例如 balance、freeze record），但用 freeze_changes/vote_changes/withdraw_changes让 Java 去补齐 Account proto 字段/动态属性。
- 计划（conformance 阶段优先级）：以 fixture 覆盖范围为准，先迁最先被纳入 fixtures 的 contract family，确保 post_db bytes 对齐。

———

## Phase 3：B-镜像（Java 仍保留 revoking，但 Rust 是权威写入）

目标：在 execution.mode=REMOTE + storage.mode=remote 时，Rust 落库，Java 不做业务语义“二次应用”，只做本地快照一致性维护，避免 flush 覆盖 Rust。

1. Java 侧必须有“总闸门”禁止 apply（否则必然 double-apply 或覆写）

- 现状：Java 有 remote.exec.apply.freeze/trc10/vote/withdraw 的开关，但 没有 applyStateChangesToLocalDatabase 的开关（framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java）。
- 计划：当 write_mode=B/persisted=true 时：
    - 跳过 applyStateChangesToLocalDatabase
    - 跳过所有可能是 delta 的 sidecar apply（TRC-10 必跳）
    - 只保留“镜像/刷新”逻辑（下一条）

2. 实现“touched-keys 镜像刷新”（B-镜像的核心）

- 问题根因：Java 在 tx 前会做资源扣费并写入 revoking snapshot，随后 ResourceSyncContext.flushPreExec()把这些值刷到 remote（framework/src/main/java/org/tron/core/db/Manager.java / framework/src/main/java/org/
  tron/core/storage/sync/ResourceSyncContext.java）。
  如果 tx 后 Java 不更新 snapshot 的最终值，SnapshotRoot.mergeWithHead(...) 仍可能把 snapshot 里的旧值写回 remote（特别是那些 Java pre-exec 触碰过的 key）。
- 计划：Rust 在返回中给出 本次执行触碰的所有 (db,key)（或直接给 (db,key,new_value)），Java 在 tx 执行后做：
    - 对每个 (db,key)：从 remote root 读最终值（getFromRoot 或 StorageSPI get），再写入 revoking head（put/delete）
    - 这样后续同 block 的读一致、flush 也不会用旧值覆盖
- 注意：这套 touched-keys 必须覆盖 EVM account/storage 以外的所有 db（account/properties/votes/witness/asset-issue/...），否则仍有“局部覆写”。

3. 配置矩阵约束（避免错误组合）

- 方案 B 只有两种合理运行方式：
    - conformance：Rust isolated DB（Java 不参与）
    - 节点：storage.mode=remote（Java 与 Rust 共享同一套 remote storage）
- storage.mode=embedded 下启用 Rust persist 基本没有意义（两边不是同一个 DB）。

4. Shadow 模式（建议直接规定“不支持方案 B 持久化”）

- execution.mode=SHADOW 会并发跑 embedded+remote（ShadowExecutionSPI），远端若持久化会污染状态并影响对比。
- 计划：Shadow 模式强制 rust_persist_enabled=false 或远端使用隔离 DB。

———

## Phase 4：从 B-镜像升级到 fullnode（含 reorg）的路线（Notes）

你选“先 B-镜像，以后再迁 rollback”，这意味着 reorg 能力一定要在某个点迁到 Rust；仅靠 Java revoking 无法回滚“已经由 Rust 写入 remote 的状态”。

升级路径 A（推荐，逐步走向 B-完全）

- 先做 block-level snapshot/rollback 在 Rust storage 里：
    - 让 Rust storage engine 支持“真实快照”（现在 create_snapshot/get_from_snapshot 在 rust-backend/crates/storage/src/engine.rs 还是占位实现）。
    - Java 在 block 生命周期钩子上：
        - beginBlock：请求 Rust CreateSnapshot（或 checkpoint id）
        - commitBlock：删除不再需要的 snapshot
        - reorg：对 Rust RevertToSnapshot，并清理/重建 Java 本地 cache 与 revoking head
- 这一步完成后，即使 Java 仍保留 revoking，也只是“本地视图/缓存”，真正的可回滚状态由 Rust 负责 —— 你就拥有 fullnode(reorg) 的基础。

升级路径 B（不太推荐，复杂度高）

- Rust 返回全量 undo log（每个 (db,key,old_bytes,new_bytes)），Java 把 undo log 按 block 记账，reorg 时用 remote storage API 写回 old_bytes。
- 这本质是在 Java 侧重新实现一套“远端可回滚 WAL”，工作量和风险都高，而且最终仍会走向“rollback 迁 Rust”。

———

## 最终演进：B-完全（Rollback 也迁 Rust）（Notes）

- Rust：统一承担 execute + persist + snapshot/reorg（block checkpoint、fast pop、fork handling）。
- Java：remote 模式下逐步降级为“执行编排 + P2P/共识 + 本地缓存”，revoking/snapshot 可在 remote 模式下弱化甚至关闭（只保留必要 cache），避免重复实现两套回滚体系。