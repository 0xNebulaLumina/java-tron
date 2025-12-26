• 我读到的现状（决定“怎么快、怎么对”）                                                                                                                                                                            
                                                                                                                                                                                                                
- Rust 非 VM 合约入口在 rust-backend/crates/core/src/service/mod.rs:195（execute_non_vm_contract，match transaction.metadata.contract_type 在 rust-backend/crates/core/src/service/mod.rs:214），未实现类型直接 
返回 error 让 Java fallback。                                                                                                                                                                                 
- Java 把合约映射成远端请求的入口在 framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:305（switch (contract.getType())），目前只覆盖少量类型，其它直接抛异常 fallback。               
- 远端结果落库（把 Rust 返回的 state_changes/sidecars 应用到 Java 本地库）在 framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:89 起；freeze/trc10/vote/withdraw 的 sidecar 应用分别在        
framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:191 等。                                                                                                                                  
- 你要“确保正确性”的最大地雷：Rust 侧 Account 编解码目前是“只写 address/type/balance/create_time”的简化版（rust-backend/crates/execution/src/storage_adapter/engine.rs:128），读也只解析 balance（rust-backend/ 
crates/execution/src/storage_adapter/engine.rs:184）。这会天然阻塞大量合约（权限、资产、unfreezeV2、market 等）正确实现；所以必须把“状态模型/codec”作为第一优先级，否则做得越快错得越多。                     
                                                                                                                                                                                                                
把未实现合约先映射到 Java“规范实现”（oracle）                                                                                                                                                                   
                                                                                                                                                                                                                
- Proposal：actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java:25 / .../ProposalApproveActuator.java:30 / .../ProposalDeleteActuator.java:28；依赖 ProposalStore（chainbase/src/main/    
java/org/tron/core/store/ProposalStore.java:19）、WitnessStore、AccountStore、DynamicPropertiesStore（例如 LATEST_PROPOSAL_NUM 在 chainbase/src/main/java/org/tron/core/store/                                
DynamicPropertiesStore.java:29）。                                                                                                                                                                            
- SetAccountId：actuator/src/main/java/org/tron/core/actuator/SetAccountIdActuator.java:20；依赖 AccountStore + AccountIdIndexStore。                                                                           
- 合约元数据更新：UpdateSetting（actuator/src/main/java/org/tron/core/actuator/UpdateSettingContractActuator.java:25）/ UpdateEnergyLimit（actuator/src/main/java/org/tron/core/actuator/                       
UpdateEnergyLimitContractActuator.java:24）/ ClearAbi（actuator/src/main/java/org/tron/core/actuator/ClearABIContractActuator.java:27）；依赖 ContractStore（chainbase/src/main/java/org/tron/core/store/     
ContractStore.java:21）+ AbiStore（chainbase/src/main/java/org/tron/core/store/AbiStore.java:18）+ 动态开关（如 ALLOW_TVM_CONSTANTINOPLE）。                                                                  
- AccountPermissionUpdate：actuator/src/main/java/org/tron/core/actuator/AccountPermissionUpdateActuator.java:28；依赖 ALLOW_MULTI_SIGN、TOTAL_SIGN_NUM、AVAILABLE_CONTRACT_TYPE、                              
UPDATE_ACCOUNT_PERMISSION_FEE（定义在 chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:29，fee getter 在 chainbase/src/main/java/org/tron/core/store/                                  
DynamicPropertiesStore.java:1661）。                                                                                                                                                                          
- UpdateBrokerage：actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java:23；依赖 DelegationStore（chainbase/src/main/java/org/tron/core/store/DelegationStore.java:25）+ witness/account +
allowChangeDelegation。                                                                                                                                                                                       
- WithdrawExpireUnfreeze / CancelAllUnfreezeV2 / DelegateResource / UnDelegateResource：                                                                                                                        
    - actuator/src/main/java/org/tron/core/actuator/WithdrawExpireUnfreezeActuator.java:28（动态开关 supportUnfreezeDelay 在 chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:2883）     
    - actuator/src/main/java/org/tron/core/actuator/CancelAllUnfreezeV2Actuator.java:34                                                                                                                         
    - actuator/src/main/java/org/tron/core/actuator/DelegateResourceActuator.java:38（动态开关 supportDR 在 chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:2021；store 名字大小写非常关
    键，如 DelegatedResourceStore 是 "DelegatedResource"：chainbase/src/main/java/org/tron/core/store/DelegatedResourceStore.java:15）                                                                        
    - actuator/src/main/java/org/tron/core/actuator/UnDelegateResourceActuator.java:31                                                                                                                          
- TRC-10 扩展：ParticipateAssetIssue（actuator/src/main/java/org/tron/core/actuator/ParticipateAssetIssueActuator.java:41）/ UnfreezeAsset（actuator/src/main/java/org/tron/core/actuator/                      
UnfreezeAssetActuator.java:27）/ UpdateAsset（actuator/src/main/java/org/tron/core/actuator/UpdateAssetActuator.java:24）。                                                                                   
- Exchange/Market/Shield：分别从 actuator/src/main/java/org/tron/core/actuator/ExchangeCreateActuator.java:30、actuator/src/main/java/org/tron/core/actuator/MarketSellAssetActuator.java:61、actuator/src/main/
java/org/tron/core/actuator/ShieldedTransferActuator.java:47 起；属于后期大项（store 多、receipt 字段多、依赖复杂/zk）。                  



Phase 0（必须先做的基础，否则“快”会变“错”）                                                                                                                                                                     

- 统一“状态读写 codec”：
    - 目标：Rust 读/写必须对齐 Java 的 protobuf（Account/Contract/Proposal/Exchange/DelegatedResource/MarketOrder…），并且更新某字段时不丢其它字段。
    - 建议做法：用 prost-build 直接编译 repo 里的 TRON proto（例如 protocol/src/main/protos/core/Tron.proto）生成 Rust 类型，然后在 EngineBackedEvmStateStore 里做 decode→modify→encode，替换当前 rust-backend/
    crates/execution/src/storage_adapter/engine.rs:128 的“自造 bytes”。
- 校准 DB 名称与 key 规则：
    - Java store 的 dbName 是事实标准（例如 proposal：chainbase/src/main/java/org/tron/core/store/ProposalStore.java:19，DelegatedResource：chainbase/src/main/java/org/tron/core/store/
    DelegatedResourceStore.java:15，大小写敏感）。
    - 把 Rust 侧所有数据库名集中成一份常量映射，避免出现类似 “account-name vs account-index” 这种不可互通的问题。
    - key 规则优先复用 capsule 的 createDbKey/calculateDbKey（例如 proposal/exchange 都是 ByteArray.fromLong：chainbase/src/main/java/org/tron/core/capsule/ProposalCapsule.java:36、chainbase/src/main/java/
    org/tron/core/capsule/ExchangeCapsule.java:37；DelegatedResource V2 key 前缀：chainbase/src/main/java/org/tron/core/capsule/DelegatedResourceCapsule.java:33）。
- 结果/回执字段盘点与协议扩展：
    - 现在 gRPC 执行结果在 framework/src/main/proto/backend.proto:589（message ExecutionResult），Java 的 ExecutionSPI 结果在 framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java:131，都没有
    覆盖 exchangeId、exchangeReceivedAmount、withdrawExpireAmount、cancelUnfreezeV2 map、shield fee 等“非状态但对外可见”的回执字段。
    - 先列一张“每个合约需要补哪些 receipt 字段”的清单；按优先级分批扩 backend.proto（framework/src/main/proto/backend.proto:501 有 ContractType 枚举），同时扩 Java 的 ExecutionSPI.ExecutionResult/
    ExecutionProgramResult 以及 Rust 的 result 转换（入口在 rust-backend/crates/core/src/service/grpc/conversion.rs:14）。

Phase 1（让所有未实现合约“先有红灯测试”，不等链上 tx）

- 建一个“用 Java 生成 golden vectors、Rust 跑 conformance”的流水线：
    - Java 侧：新增一个 test/工具，针对每个合约类型构造最小链状态（小 DB、少 key），跑 embedded actuator 得到 post-state；把 pre/post 的“相关 DB 全量 kv dump”+ 合约参数 bytes + context + 期望 receipt 输出成
    JSON/二进制 fixture（小状态时全量 dump 很便宜，不需要复杂的读集跟踪）。
    - Rust 侧：读 fixture，把 kv 写入 StorageEngine，调用 execute_non_vm_contract（rust-backend/crates/core/src/service/mod.rs:195）执行，然后 dump 同样的 DB 并逐字节比对。
    - 对比维度：优先“最终 DB bytes + receipt 字段”；state_changes 可选用 digest（Java 已有 StateChangeCanonicalizer.computeStateDigest：framework/src/main/java/org/tron/core/execution/reporting/
    StateChangeCanonicalizer.java:39，Domain digest 在 framework/src/main/java/org/tron/core/execution/reporting/DomainCanonicalizer.java:42）。
- 复用你现有的全链对比工具做“回归/大样本”：
    - collect_remote_results.sh:1 + scripts/compare_exec_csv.py:18 已经能做 embedded-vs-remote 的 CSV diff；把它定位为 nightly/长跑，fixture conformance 定位为 PR/日常快速迭代。

Phase 2（按依赖分组实现：先小依赖，后大依赖）

- 组 A（最快见效）：Proposal 16/17/18
    - 依赖：ProposalStore + WitnessStore + DynamicPropertiesStore；几乎不碰复杂 account 字段。
    - 同步改动：RemoteExecutionSPI 增加 case（framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:305）传 full proto bytes；Rust 加 feature flag + match arm；必要时 proto 扩
    receipt（大概率不需要）。
- 组 B：SetAccountId 19、AccountPermissionUpdate 46
    - 依赖：Account protobuf 完整读写 + AccountIdIndexStore；权限更新还依赖 dynamic props（UPDATE_ACCOUNT_PERMISSION_FEE 等）。
    - 这里能验证你“Account codec 修复”是否到位。
- 组 C：UpdateSetting 33 / UpdateEnergyLimit 45 / ClearAbi 48
    - 依赖：ContractStore/AbiStore protobuf 读写 + 动态开关（如 ALLOW_TVM_CONSTANTINOPLE）。
- 组 D：WithdrawExpireUnfreeze 56 / CancelAllUnfreezeV2 59 / DelegateResource 57 / UnDelegateResource 58
    - 依赖：Account 的 unfrozenV2/frozenV2/delegation 字段、DelegatedResource* store（注意 dbName 大小写）、以及资源处理器逻辑；必要时扩 backend.proto 补 withdrawExpireAmount/cancel map。
- 组 E：TRC-10 扩展 9/14/15
    - 依赖：Account 的 asset map + AssetIssueStore/V2 + allowSameTokenName 分支；可能需要扩 Trc10Change oneof（framework/src/main/proto/backend.proto:742）来表达 Participate/Update/UnfreezeSupply 这种语义变
    化，或者直接让 Rust 端完整落库（取决于你是否要继续“Rust 只算、Java 落库”的分工）。
- 组 F/G（后期）：Exchange 41-44、Market 52-53
    - 大量 store + 大量 receipt 字段（exchangeId/receivedAmount/anotherAmount/orderId 等），建议等 Phase 0 的“proto 扩展机制”和“codec 层”稳定后再上。
- 组 H（最后）：Shield 51
    - 需要 zk/merkle/nullifier 等全套依赖，工作量与风险远大于其它类型；建议独立里程碑。

Phase 3（灰度与 CI）

- 灰度开关双层化：
    - Rust：继续沿用 rust-backend/config.toml 的 [execution.remote] per-contract flags（同类逻辑已在 rust-backend/crates/core/src/service/mod.rs:195 使用）。
    - Java：RemoteExecutionSPI 也用 JVM property 做开关（现有 remote.exec.trc10.enabled 模式），先“可回滚”，再默认开启。
- Shadow/对比：
    - ShadowExecutionSPI 现在比较很粗（入口在 framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java:355），建议直接接入 StateChangeCanonicalizer/DomainCanonicalizer 做 digest 级别对比，
    作为线上/长跑的安全网。
- CI 建议：
    - PR：跑 fixture conformance（小样本、覆盖所有未实现合约的 happy/validate-fail/edge 三类用例）。
    - Nightly：跑 collect_remote_results.sh 这种全链/长时间回放对比，抓真实区块回归。

如果你愿意，我可以按上面的 Phase 0 先帮你把“Rust 侧 proto 编解码 + DB 名称/Key 对齐 + fixture conformance 框架”细化到具体目录结构、fixture schema、以及每个 contract 的最小 pre-state 清单。

