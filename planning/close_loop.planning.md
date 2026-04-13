• 我先把结论说清楚：你现在这条路不是“从零重写一个 Rust 节点”，而是“保留 Java 外壳，逐步把核心内核迁到 Rust”。按这个目标看，storage 算是主路径已经打通，但还不能算完整替代；smart contract 的写交易主链路已经很强，
  但如果按“完整节点合约能力”来算，还没闭环。下一步我不建议直接做 P2P / 区块同步。                                                                                                                                 
                                                                                                                                                                                                                  
  现状判断                                                                                                                                                                                                        
                                                                                                                                                                                                                  
  - 架构方向是对的。Java 侧已经把存储和执行都做成可切换的 SPI，而且 ADR 也明确把 Rust backend 定位成将来承载 storage/execution/consensus/p2p 的统一进程。framework/src/main/java/org/tron/core/storage/spi/       
    StorageSpiFactory.java:60 framework/src/main/java/org/tron/core/execution/spi/ExecutionSpiFactory.java:79 docs/ADR-012-unified-rust-backend.md:16                                                             
  - storage：                                                                                                                                                                                                     
      - 热路径是真的有了。Rust 侧已经有 RocksDB 的 get/put/delete/batch_write/batch_get。rust-backend/crates/storage/src/engine.rs:189                                                                            
      - 但语义没封口。snapshot 现在只是回读当前库，不是真快照。rust-backend/crates/storage/src/engine.rs:460                                                                                                      
      - 事务也是半成品。proto 已经有 transaction_id，framework/src/main/proto/backend.proto:110 但 Java RemoteStorageSPI 的 put/delete/batchWrite 没传这个字段，framework/src/main/java/org/tron/core/storage/spi/
        RemoteStorageSPI.java:138 framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java:209 Rust gRPC 端也直接写库。rust-backend/crates/core/src/service/grpc/mod.rs:192                        
      - 所以我的判断是：storage 可以说“基本可用”，不能说“基本完成”。                                                                                                                                              
  - smart contract / execution：                                                                                                                                                                                  
      - 如果你说的是“上链写交易执行主路径”，这块已经远超 basic。Java 已经能接 Rust 返回的 state changes、freeze/resource、TRC-10、vote、withdraw sidecar 并应用回本地状态。framework/src/main/java/org/tron/      
        common/runtime/RuntimeSpiImpl.java:93                                                                                                                                                                     
      - Java RemoteExecutionSPI 已经映射了大量合约类型到 Rust。framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:403                                                                    
      - Rust 后端也已经有大批系统合约分发和 feature flag。rust-backend/crates/core/src/service/mod.rs:647                                                                                                         
      - 但这还是桥接态，不是闭环态。ResourceSyncService 还在专门把 Java 侧资源变更预同步到远端存储，这说明状态所有权仍然是分裂的。framework/src/main/java/org/tron/core/storage/sync/ResourceSyncService.java:31  
      - 节点级合约能力没闭环。Java 侧 callContract/estimateEnergy/getCode/getStorageAt/getNonce/getBalance/snapshot/health 还是 placeholder。framework/src/main/java/org/tron/core/execution/spi/                 
        RemoteExecutionSPI.java:120                                                                                                                                                                               
      - Rust gRPC 虽然已经实现了 call_contract 和 estimate_energy，rust-backend/crates/core/src/service/grpc/mod.rs:1644 rust-backend/crates/core/src/service/grpc/mod.rs:1716 但 get_code/get_storage_at/        
        get_nonce/get_balance/create_evm_snapshot/revert_to_evm_snapshot 还是占位。rust-backend/crates/core/src/service/grpc/mod.rs:1802                                                                          
      - 还有两个关键缺口：energy_limit 线协议语义已知有 mismatch，rust-backend/crates/execution/src/lib.rs:113 TriggerSmartContract 的 TRC-10 pre-transfer 也明确还没实现。rust-backend/crates/execution/src/     
        lib.rs:515                                                                                                                                                                                                
      - 所以我的判断是：如果按“写交易执行主链路”算，smart contract 已经相当成熟；如果按“完整节点能力”算，还不能说基本完成。                                                                                       
                                                                                                                                                                                                                  
  为什么不建议现在直接做 P2P                                                                                                                                                                                      
                                                                                                                                                                                                                  
  - P2P 在这里不是一个薄模块。网络层启动就会拉起 adv/sync/fetchblock/relay/effective check 等一串子系统。framework/src/main/java/org/tron/core/net/TronNetService.java:87                                         
  - 消息分发也很厚，包含 handshake、inventory、sync、block、pbft 等多路分流。framework/src/main/java/org/tron/core/net/P2pEventHandlerImpl.java:144                                                               
  - 握手里就有版本、genesis、solid block、head block 兼容性检查。framework/src/main/java/org/tron/core/net/service/handshake/HandshakeService.java:36                                                             
  - 同步服务本身也有 fork summary、请求调度、去重、抓取、超时状态机。framework/src/main/java/org/tron/core/net/service/sync/SyncService.java:72                                                                   
  - 更关键的是，块一进来就走 TronNetDelegate.processBlock -> Manager.pushBlock -> Manager.processBlock -> consensus.applyBlock/maintenance 这条核心状态机。framework/src/main/java/org/tron/core/net/TronNetDeleg
    ate.java:236 framework/src/main/java/org/tron/core/db/Manager.java:1257 framework/src/main/java/org/tron/core/db/Manager.java:1906 consensus/src/main/java/org/tron/consensus/dpos/DposService.java:152 conse
    nsus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:94
  - 所以现在先做 P2P，等于把最嘈杂的边缘和最核心的状态机问题绑在一起做，风险最高。

  可选路线

  - 路线 A：先补 execution + storage 闭环。
      - 优点：最直接解决正确性问题，且你已经有 SHADOW 和 replay 基础设施可用。framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java:14 framework/src/test/java/org/tron/core/execution/spi/
        HistoricalReplayTool.java:17
      - 缺点：短期看不到 Rust 网络栈进度。
      - 成本：中。
  - 路线 B：先做 Rust block importer / state transition engine，Java P2P 先不动。
      - 优点：这是最接近 full Rust node 核心的下一步。
      - 缺点：会开始深入碰 Manager、consensus、maintenance，复杂度高于单纯补 execution。
      - 成本：中高。
  - 路线 C：直接做 Rust P2P / sync。
      - 优点：表面上最像“全面替换”。
      - 缺点：收益最小，风险最大。
      - 成本：高。

  我的建议

  - 现在不要直接做 P2P 同步区块。
  - 你下一步最应该做的是：先把 Rust 做成“可独立负责区块状态转移的内核”，再去替换网络入口。
  - 更具体地说，下一里程碑应该是 Rust Block Executor/Importer，不是 Rust P2P。

  推荐计划

  - Phase 1：补 execution/storage 闭环。
      - 锁定唯一写入权威。现在 config.toml 开了很多 aggressive feature，并且 rust_persist_enabled = true，但代码默认和注释都在暗示安全路径应当是 false。rust-backend/config.toml:63 rust-backend/config.toml:156
        rust-backend/crates/common/src/config.rs:587
      - 把 Java callContract/estimateEnergy 接到 Rust 已有 gRPC 实现上，同时补完 getCode/getStorageAt/getNonce/getBalance/snapshot。
      - 锁定 energy_limit 的 wire spec。
      - 完成 TriggerSmartContract 的 TRC-10 pre-transfer。
      - 要么做真事务/真快照，要么把现在的假事务/假快照接口显式降级。
      - 给 storage crate 补真正的 Rust 单测。现在 cargo test -p tron-backend-storage 通过了，但其实是 0 tests；Java 侧虽有双模集成测试，信心还不够。framework/src/test/java/org/tron/core/storage/spi/
        DualStorageModeIntegrationTest.java:19
  - Phase 2：抽 Rust block importer。
      - 按 framework/src/main/java/org/tron/core/db/Manager.java:1906 的职责拆 block apply 主循环：tx 顺序执行、receipt、block energy、proposal、reward、dynamic properties、recent block/tx 索引。
      - Java 先继续负责收块，Rust 负责“给我一个 block，我 deterministically apply 掉”。
      - 这一阶段验收标准不是“能联网”，而是“同一批历史块，Rust apply 的结果和 Java 一致”。
  - Phase 3：迁移 consensus / maintenance。
      - 重点是 DposService + MaintenanceManager：witness 调度、vote 结算、active witness 更新、delegation/brokerage cycle、solid block 更新。consensus/src/main/java/org/tron/consensus/dpos/DposService.java:72
        consensus/src/main/java/org/tron/consensus/dpos/MaintenanceManager.java:94
      - 只有这部分稳定后，Rust 才真正拥有链状态机所有权。
  - Phase 4：最后再做 sync/P2P。
      - 第一小步不是 full replacement，而是 Rust passive sync client / observer。
      - 第二小步才是 handshake、peer 管理、inventory、fetch、广播、重连等全量替换。
      - 到这一步再拿掉 Java 的 libp2p 依赖才合理。common/build.gradle:47

  我还核对了当前测试信号：cargo test -p tron-backend-core create_smart_contract -- --nocapture 和 cargo test -p tron-backend-core update_setting -- --nocapture 都是 17/17 通过；cargo test -p tron-backend-
  storage -- --nocapture 通过，但该 crate 目前是 0 tests。

  如果你要，我下一轮可以直接把 Phase 1 拆成一个更细的 WBS：按模块、依赖、风险、验收标准和优先级排成执行清单。

---

• 我把它按项目执行来拆，不按源码目录拆。重点会放在三层：关键路径、可并行工作流、每个工作包的验收标准，不展开实现细节。                                                                                            
                                                                                                                                                                                                                  
• 那我就按“项目落地执行”的方式拆，不按目录拆。                                                                                                                                                                    
  你现在最该拆的是我上条里说的 Phase 1：execution/storage 闭环，因为这是后面 block importer / consensus / sync 的前置条件。                                                                                       
                                                                                                                                                                                                                  
  我建议把它拆成 1 个总目标 + 5 个工作流 + 3 个里程碑。                                                                                                                                                           
                                                                                                                                                                                                                  
  总目标                                                                                                                                                                                                          
  把当前“Java 壳 + Rust backend”的执行链路，从“可跑部分写交易”推进到“可作为区块状态转移内核使用”。                                                                                                                
                                                                                                                                                                                                                  
  验收口径不是“功能点又多了几个”，而是这 4 条：                                                                                                                                                                   
                                                                                                                                                                                                                  
  - Rust 对执行语义的定义一致，不再有明显 wire mismatch。                                                                                                                                                         
  - Java/Rust 的状态所有权清晰，不再双写、半写、补丁式同步。                                                                                                                                                      
  - 读路径和回滚/快照能力闭环，能支撑 call/estimate/replay/shadow。                                                                                                                                               
  - 有可持续的 parity 验证手段，不靠人工 spot check。                                                                                                                                                             
                                                                                                                                                                                                                  
  ———                                                                                                                                                                                                             
                                                                                                                                                                                                                  
  工作流 A：先锁语义，不先写功能                                                                                                                                                                                  
  这是最优先的 P0，必须先做。                                                                                                                                                                                     
                                                                                                                                                                                                                  
  目标：                                                                                                                                                                                                          
  把现在最危险的“看起来接好了，实际上两边含义不同”的地方先定死。                                                                                                                                                  
                                                                                                                                                                                                                  
  范围：                                                                                                                                                                                                          
                                                                                                                                                                                                                  
  - energy_limit 线协议语义定版。rust-backend/crates/execution/src/lib.rs:113                                                                                                                                     
  - 写入权威定版：到底是 Java apply 为准，还是 Rust persist 为准。framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:93 rust-backend/config.toml:156                                             
  - transaction/snapshot 语义定版：是做真事务/真快照，还是短期明确降级。                                                                                                                                          
  - callContract / estimateEnergy / getCode / getStorageAt / getNonce / getBalance 的语义定义统一。framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:120                                
                                                                                                                                                                                                                  
  产出：                                                                                                                                                                                                          
                                                                                                                                                                                                                  
  - 一份迁移语义说明，最好直接落在 docs/ 或 planning 文档里。                                                                                                                                                     
  - 一张接口矩阵：每个 RPC 当前状态、目标状态、单位、回滚语义、错误语义。                                                                                                                                         
                                                                                                                                                                                                                  
  验收标准：                                                                                                                                                                                                      
                                                                                                                                                                                                                  
  - energy_limit 不再存在“Java 发的是 energy units，Rust 当成 SUN 再除一次”的歧义。                                                                                                                               
  - 团队内部能明确回答：rust_persist_enabled 在生产验收阶段默认应为 true 还是 false。                                                                                                                             
  - 每个未完成 API 都有明确“补齐”或“显式禁用”的处理，不留灰区。                                                                                                                                                   
                                                                                                                                                                                                                  
  风险：                                                                                                                                                                                                          
                                                                                                                                                                                                                  
  - 这是最容易被跳过的一步，但不做，后面所有测试结果都不可信。                                                                                                                                                    
                                                                                                                                                                                                                  
  ———                                                                                                                                                                                                             
                                                                                                                                                                                                                  
  工作流 B：补 execution 读路径闭环                                                                                                                                                                               
  这是第二个 P0，建议紧跟 A。                                                                                                                                                                                     
                                                                                                                                                                                                                  
  为什么先做它：
  因为你现在写路径已经不弱了，但读路径还大量 placeholder，这会直接卡住 shadow、replay、RPC、调试、估算和诊断。

  当前缺口：

  - Java 侧 placeholder：callContract / estimateEnergy / getCode / getStorageAt / getNonce / getBalance / snapshot / health。framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:120
  - Rust gRPC 已有 call_contract / estimate_energy，但其余读接口还是未实现。rust-backend/crates/core/src/service/grpc/mod.rs:1644 rust-backend/crates/core/src/service/grpc/mod.rs:1802

  建议拆成 3 个子包：

  1. B1：Java RemoteExecutionSPI 接通已存在的 Rust call/estimate
  2. B2：Rust 实现 getCode/getStorageAt/getNonce/getBalance
  3. B3：EVM snapshot/revert 语义补齐或显式下线

  依赖：

  - 依赖工作流 A 的语义冻结。
  - 不依赖 storage 事务完全做完，可以先基于只读路径推进。

  验收标准：

  - callContract 不再返回 placeholder。
  - estimateEnergy 和 embedded 的偏差进入可量化范围。
  - getCode/getStorageAt/getNonce/getBalance 对常见合约与账户可正确返回。
  - snapshot 如果短期不做真快照，就必须在接口层明确不可用，而不是返回伪成功。

  建议优先级：

  - callContract
  - estimateEnergy
  - getCode/getStorageAt
  - getNonce/getBalance
  - snapshot/revert

  ———

  工作流 C：补 storage 语义完整性
  这是第三个 P0，但它不是“先把所有 DB 能力做满”，而是“把 execution 真正依赖的那部分做可靠”。

  当前真实状态：

  - CRUD/batch 已经成形。rust-backend/crates/storage/src/engine.rs:189
  - snapshot 是假的。rust-backend/crates/storage/src/engine.rs:460
  - transaction 只有 begin/commit/rollback 壳子，写操作没进 transaction buffer。rust-backend/crates/storage/src/engine.rs:387
  - proto 其实已经留了 transaction_id，但 Java/Rust 写路径没用起来。framework/src/main/proto/backend.proto:110 framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java:138

  建议拆成 4 个子包：

  1. C1：transaction_id 真正贯通
  2. C2：事务内写缓冲 + commit/rollback 生效
  3. C3：真 snapshot 或明确禁用 snapshot
  4. C4：execution 依赖的 DB 一致性测试

  两种可选方案：

  - 方案 1：短期务实
      - 先不做跨 DB 真事务。
      - 明确 remote execution 运行时只依赖单次执行隔离，不暴露通用事务承诺。
      - 优点是快。
      - 缺点是未来 block importer 还要返工。                                                                                                                                                   07:47:46 [117/1378]
  - 方案 2：中期正确
      - 直接把 transaction/snapshot 做成可用于 execution/block apply 的真实语义。
      - 优点是后续能直接承接 block importer。
      - 缺点是现在成本更高。

  我的建议：

  - 走 1.5 路线。
  - 先把 execution 实际依赖的事务边界 做对，不急着把 StorageSPI 全部变成“通用数据库产品”。

  验收标准：

  - beginTransaction -> put/delete/batchWrite -> commit/rollback 真正改变结果。
  - snapshot 的读取在创建后具备时间点一致性；如果做不到，就禁止 execution 依赖它。
  - Java RemoteStorageSPI 和 Rust backend 对事务字段的使用一致。

  ———

  工作流 D：补执行边角语义，不让“局部已支持”误导整体判断
  这是 P1，不是最先做，但一定要在 Phase 1 内完成。

  明确缺口：

  - TriggerSmartContract 的 TRC-10 pre-execution transfer 还没实现。rust-backend/crates/execution/src/lib.rs:515
  - 现在很多 system contract 开关在 config.toml 里全开，但代码默认值仍是保守模式，这说明当前更像试验性 rollout，而不是稳定基线。rust-backend/config.toml:63 rust-backend/crates/common/src/config.rs:587

  建议拆成 3 个子包：

  1. D1：VM 交易的资源/energy/fee 边界统一
  2. D2：TRC-10 与 VM 交叉路径补齐
  3. D3：feature flag 收敛，区分实验开关与默认生产开关

  验收标准：

  - 所有“当前仅靠 config 开启才敢跑”的能力，都有明确的稳定性标签。
  - 至少列出一份“可进入 shadow/replay 的 contract 类型白名单”。
  - 明确哪些合约类型允许 remote canonical，哪些只能 shadow/fallback。

  ———

  工作流 E：建立 parity 验证体系
  这是 P0/P1 交界项，建议尽早并行推进。

  你的好消息是，这套基础设施已经有了：

  - SHADOW 模式已经存在。framework/src/main/java/org/tron/core/execution/spi/ShadowExecutionSPI.java:20
  - HistoricalReplayTool 已经有雏形。framework/src/test/java/org/tron/core/execution/spi/HistoricalReplayTool.java:17
  - Rust core 侧也已经有不少合约测试。
  - 但 storage crate 目前 0 tests，这是明显短板。

  建议拆成 4 个子包：

  1. E1：storage crate 基础单测
  2. E2：execution 读路径与写路径分开验收
  3. E3：golden vectors 真正接入执行，不只是框架占位
  4. E4：历史区块 replay 形成日报/周报指标

  建议指标：

  - shadow mismatch rate
  - 按 contract type 分类 mismatch
  - estimateEnergy 偏差率
  - storage transaction/snapshot 失败率
  - read-path API 覆盖率

  验收标准：

  - 不是“测试能跑”，而是“测试能告诉你还能不能继续迁移”。
  - 每周都能回答：哪些 contract type 已达 parity，哪些还不能进入 canonical。

  ———

  关键路径
  真正的关键路径只有这一条：

  1. 语义冻结
  2. execution 读路径补齐
  3. storage 事务/快照做到 execution 可依赖
  4. parity 验证打通
  5. 再进入 block importer

  也就是说：

  - A -> B -> C -> E 是主线
  - D 可以插在 B/C 之间并行推进
  - P2P 不在当前关键路径上

  ———

  可并行拆法
  如果你是按人力分工，我建议这样拆：

  - 负责人 1：协议与 Java 桥接
      - 负责工作流 A、B
      - 重点文件：RemoteExecutionSPI、proto、ExecutionSPI 工厂/模式、health/read APIs
  - 负责人 2：Rust execution 语义
      - 负责工作流 D + B 的 Rust 部分
      - 重点文件：execution/src/lib.rs、core/src/service/grpc/mod.rs
  - 负责人 3：Rust storage 语义
      - 负责工作流 C
      - 重点文件：storage/src/engine.rs、RemoteStorageSPI、storage proto 对应通路
  - 负责人 4：验证与回归
      - 负责工作流 E
      - 重点文件：ShadowExecutionSPI、HistoricalReplayTool、golden vectors、Rust tests

  这样拆的好处是写集冲突小，而且每个人都有独立验收口。

  ———

  建议里程碑
  我建议 Phase 1 分成 3 个里程碑，而不是一次做完。

  1. M1：接口不再假实现

  - Java execution 读路径 placeholder 基本消失
  - Rust gRPC 读接口不再大面积 Not implemented
  - energy_limit 语义冻结

  2. M2：状态语义可信

  - storage transaction/snapshot 不再名不副实
  - rust_persist_enabled 策略明确
  - TRC-10/VM 交叉路径补齐主要缺口

  3. M3：可以做 block importer

  - shadow/replay 有稳定指标
  - 一批目标 contract type 达到可接受 parity
  - 可以开始抽块执行主循环，而不是继续堆零散合约支持

  ———

  时间和优先级建议
  如果按 4 个人并行、以“能推进下一阶段”为目标，不追求一次性完美，我会这样排：

  - 第 1 周：
      - A 全做完
      - B1 开始
      - E1/E2 起步
  - 第 2 到 3 周：
      - B2/B3
      - C1/C2
      - D1
  - 第 4 周：
      - C3/C4
      - D2/D3
      - E3/E4
  - 第 5 周：
      - 收敛 mismatch
      - 定义可进入 block importer 的 contract 白名单
      - 评审是否进入下一阶段

  ———

  你下一步最实用的动作
  不是开写 P2P，而是先把这 5 个工作流列成 issue/epic。

  我建议 issue 层级这样建：

  - Epic 1: Remote execution semantics freeze
  - Epic 2: Remote execution read-path closure
  - Epic 3: Storage transaction/snapshot correctness 
  - Epic 4: VM/TRC-10/resource parity closure
  - Epic 5: Shadow/replay verification pipeline

---

• 我把它整理成可直接落到 GitHub Issues/Jira 的粒度，按 Epic -> Issue 给出标题、目标、依赖、验收和建议 owner。重点仍然是先完成 Phase 1，不把 P2P 提前拉进关键路径。                                                
                                                                                                                                                                                                                  
• 下面这版可以直接转成 GitHub Issues/Jira。默认目标是：先把 Rust backend 做成可信的状态转移内核，再进入 block importer / consensus / sync，当前不把 P2P 放进本阶段关键路径。                                      
                                                                                                                                                                                                                  
  Epic 1                                                                                                                                                                                                          
                                                                                                                                                                                                                  
  1. ARCH-01 冻结执行写入权威模型，P0。目标：明确 EMBEDDED / REMOTE / SHADOW 下，谁是唯一写入权威，是否允许 rust_persist_enabled=true，以及 Java apply 与 Rust persist 的关系；关键文件：framework/src/main/java/ 
     org/tron/common/runtime/RuntimeSpiImpl.java:93 rust-backend/config.toml:156 rust-backend/crates/common/src/config.rs:606。依赖：无。验收：产出一页真值表，明确 SHADOW 不允许污染 canonical state，默认配置与 
     文档一致。建议 owner：架构负责人 + Java bridge 负责人。                                                                                                                                                      
  2. ARCH-02 冻结 energy_limit 线协议语义，P0。目标：统一 Java 发送的是 SUN 还是 energy units，并消除 Rust 端再次换算的歧义；关键文件：framework/src/main/java/org/tron/core/execution/spi/                       
     RemoteExecutionSPI.java:521 rust-backend/crates/execution/src/lib.rs:113。依赖：无。验收：协议说明更新，Java/Rust/fixture 三边一致，estimateEnergy 与 execute 使用同一单位定义。建议 owner：Java execution   
     owner + Rust execution owner。                                                                                                                                                                               
  3. ARCH-03 冻结 transaction/snapshot 语义合同，P0。目标：决定 storage transaction 和 snapshot 是短期做成真实能力，还是显式降级为“不保证语义”；关键文件：framework/src/main/proto/backend.proto:110 framework/   
     src/main/java/org/tron/core/storage/spi/StorageSPI.java:53 rust-backend/crates/storage/src/engine.rs:387。依赖：无。验收：不再出现“接口看起来支持，实际语义是假的”状态；每个接口都有明确 SLA。建议 owner：架 
     构负责人 + Rust storage owner。                                                                                                                                                                              
  4. ARCH-04 建 remote contract 支持矩阵，P0。目标：把合约类型分成 canonical remote、shadow only、Java only 三层，并把 feature flag 与支持层级对应起来；关键文件：framework/src/main/java/org/tron/core/execution/
     spi/RemoteExecutionSPI.java:403 rust-backend/crates/core/src/service/mod.rs:647 rust-backend/config.toml:63。依赖：ARCH-01。验收：有一份白名单表，配置默认值与这份表一致。建议 owner：项目 owner + execution 
     owners。                                                                                                                                                                                                     
                                                                                                                                                                                                                  
  Epic 2                                                                                                                                                                                                          
                                                                                                                                                                                                                  
  1. EXEC-01 接通 Java callContract / estimateEnergy 到 Rust 已有 RPC，P0。目标：去掉 Java 侧这两个 placeholder，让它们真的走 Rust gRPC；关键文件：framework/src/main/java/org/tron/core/execution/spi/           
     RemoteExecutionSPI.java:120 rust-backend/crates/core/src/service/grpc/mod.rs:1644。依赖：ARCH-02。验收：Java 侧不再返回占位结果，SHADOW 可对 call/estimate 做真实比对。建议 owner：Java bridge owner。       
  2. EXEC-02 实现 Rust execution query APIs，P0。目标：补齐 getCode / getStorageAt / getNonce / getBalance，形成最小可用读路径；关键文件：rust-backend/crates/core/src/service/grpc/mod.rs:1802。依赖：ARCH-01、  
     ARCH-03。验收：对常见 EOA 与 contract 查询可返回正确值，Java 侧不再需要空数组/0 的占位回退。建议 owner：Rust execution owner。                                                                               
  3. EXEC-03 处理 execution snapshot/revert/health 语义，P0。目标：补齐 createEvmSnapshot / revertToEvmSnapshot / healthCheck，或明确禁用并在 Java 侧可观测失败；关键文件：framework/src/main/java/org/tron/core/ 
     execution/spi/RemoteExecutionSPI.java:214 rust-backend/crates/core/src/service/grpc/mod.rs:1870。依赖：ARCH-03。验收：不能再返回“伪成功”快照或恒定 unhealthy health。建议 owner：Rust execution owner + Java 
     bridge owner。

  Epic 3

  1. STOR-01 贯通 transaction_id 到真实写路径，P0。目标：让 Java RemoteStorageSPI 的 put/delete/batchWrite 传递 proto 已存在的 transaction_id，Rust gRPC 和 engine 真正识别它；关键文件：framework/src/main/proto/
     backend.proto:110 framework/src/main/java/org/tron/core/storage/spi/RemoteStorageSPI.java:138 rust-backend/crates/core/src/service/grpc/mod.rs:192。依赖：ARCH-03。验收：事务内写操作不再直接落盘。建议
     owner：Java storage owner + Rust storage owner。
  2. STOR-02 实现 Rust storage 事务缓冲与 commit/rollback，P0。目标：begin -> writes -> commit/rollback 真正影响结果，而不是空事务壳；关键文件：rust-backend/crates/storage/src/engine.rs:387。依赖：STOR-01。验
     收：新增事务单测，commit 可见、rollback 不可见，事务外读行为有明确语义。建议 owner：Rust storage owner。
  3. STOR-03 实现真 snapshot 或显式禁止 snapshot，P0。目标：消除当前“snapshot 实际读当前库”的伪语义；关键文件：rust-backend/crates/storage/src/engine.rs:460。依赖：ARCH-03。验收：要么是真时间点一致性，要么接口
     明确返回 unsupported，不允许伪快照继续存在。建议 owner：Rust storage owner。
  4. STOR-04 建 storage crate 测试与故障注入基线，P0。目标：补齐当前 storage crate 0 tests 的明显空洞；关键文件：rust-backend/crates/storage/Cargo.toml rust-backend/crates/storage/src/engine.rs。依赖：STOR-02、
     STOR-03。验收：至少覆盖 CRUD、batch、transaction、snapshot、并发边界和异常路径。建议 owner：Rust storage owner + QA/verification owner。

  Epic 4

  1. PAR-01 补齐 TriggerSmartContract 的 TRC-10 pre-transfer 语义，P1。目标：实现 Java VMActuator.call() 对应的 token pre-exec transfer，消除当前显式拒绝路径；关键文件：rust-backend/crates/execution/src/
     lib.rs:515。依赖：ARCH-02、STOR-01。验收：tokenValue > 0 的 trigger contract 不再被强制拒绝，且状态与 Java 一致。建议 owner：Rust execution owner。
  2. PAR-02 收敛资源/手续费/energy 记账 parity，P1。目标：统一 energy, bandwidth, fee, freeze/resource ledger, global resource totals 的跨引擎语义；关键文件：framework/src/main/java/org/tron/common/runtime/
     RuntimeSpiImpl.java:107 rust-backend/config.toml:95。依赖：ARCH-02、EXEC-01。验收：选定的 contract 白名单在 energy_used、sidecars、账户资源结果上进入可接受偏差范围。建议 owner：Rust execution owner + Java
     runtime owner。
  3. PAR-03 收敛迁移期开关并定义桥接件退出计划，P1。目标：梳理 ResourceSyncService、feature flags、aggressive config 与默认值的关系，定义哪些是临时桥接，哪些会保留；关键文件：framework/src/main/java/org/tron/
     core/storage/sync/ResourceSyncService.java:31 rust-backend/config.toml:63 rust-backend/crates/common/src/config.rs:587。依赖：ARCH-01、ARCH-04。验收：每个开关都有状态标签：实验、shadow-only、canonical-
     ready、待删除。建议 owner：项目 owner + execution/storage owners。

  Epic 5

  1. VER-01 让 golden vectors 真的执行 remote/shadow 路径，P0。目标：把现在偏框架性的 golden vector 测试变成真实执行验证；关键文件：framework/src/test/java/org/tron/core/execution/spi/
     GoldenVectorTestSuite.java:70。依赖：EXEC-01、EXEC-02。验收：golden vectors 能输出真实 mismatch，而不是只验证结构。建议 owner：verification owner。
  2. VER-02 建历史区块 replay 的最小闭环，P0。目标：让 HistoricalReplayTool 对一段固定 block range 出真实报告，而不是只作为雏形工具存在；关键文件：framework/src/test/java/org/tron/core/execution/spi/
     HistoricalReplayTool.java:17。依赖：EXEC-01、PAR-02。验收：可稳定输出 block 数、tx 数、mismatch 数、按 contract 分类的失败报告。建议 owner：verification owner。
  3. VER-03 建 contract-type parity 看板和准入门槛，P0。目标：把 remote contract 支持从“凭感觉开开关”变成“按 contract type 通过验收后入白名单”；关键依赖：framework/src/main/java/org/tron/core/execution/spi/
     ShadowExecutionSPI.java:71 rust-backend/config.toml:63。依赖：ARCH-04、VER-01、VER-02。验收：每类 contract 都有状态：未测、shadow 通过、可 canonical、禁止 remote。建议 owner：项目 owner + QA owner。
  4. VER-04 建 CI 级 remote/shadow smoke gate，P1。目标：至少在 CI 中稳定跑一组 storage、execution、shadow smoke tests，避免继续无感回退；依赖：framework/src/test/java/org/tron/core/storage/spi/
     DualStorageModeIntegrationTest.java:19。依赖：STOR-04、VER-01。验收：PR 能看到 remote/shadow 基线是否被打破。建议 owner：infra/QA owner。

  推荐排期

  1. Sprint 1：ARCH-01 ARCH-02 ARCH-03 ARCH-04 EXEC-01 STOR-01
  2. Sprint 2：EXEC-02 EXEC-03 STOR-02 STOR-03
  3. Sprint 3：STOR-04 PAR-01 PAR-02 PAR-03
  4. Sprint 4：VER-01 VER-02 VER-03 VER-04

  Phase 1 出口条件

  - Java execution 读路径不再有 placeholder。
  - storage transaction/snapshot 不再是伪语义。
  - energy_limit、写入权威、feature flag 分层全部冻结。
  - 至少一批高价值 contract type 达到 shadow stable，并形成 canonical remote 白名单。
  - CI 和 replay 能持续告诉你“现在能不能进下一阶段”。

  下一阶段的第一个 Epic

  - 不是 P2P-01。
  - 应该是 BLOCK-01 Rust Block Importer：把 framework/src/main/java/org/tron/core/db/Manager.java:1906 这一层的 block apply 主循环抽到 Rust，先保留 Java 网络入口。

---

• 我继续往下拆到“可以直接分人开工”的粒度。会保留你现在的主线判断：先把 execution/storage 收口，再进 block importer，不把 p2p 提前拉上关键路径。                                                                   
                                                                                                                                                                                                                  
• 下面这版我直接拆到“可以建子任务、分人、排 Sprint”的粒度。                                                                                                                                                       
  默认团队角色我先假设 5 类：                                                                                                                                                                                     
                                                                                                                                                                                                                  
  - A：架构/项目 owner                                                                                                                                                                                            
  - JX：Java execution / bridge                                                                                                                                                                                   
  - JS：Java storage / bridge                                                                                                                                                                                     
  - RX：Rust execution                                                                                                                                                                                            
  - RS：Rust storage                                                                                                                                                                                              
  - V：验证/QA/CI                                                                                                                                                                                                 
                                                                                                                                                                                                                  
  如果你团队人少，可以一个人兼两类。                                                                                                                                                                              
                                                                                                                                                                                                                  
  Epic 1：语义冻结                                                                                                                                                                                                
                                                                                                                                                                                                                  
  ARCH-01 冻结执行写入权威模型                                                                                                                                                                                    

  1. 列出 EMBEDDED / REMOTE / SHADOW 三种模式下，谁负责最终持久化。
  2. 列出 rust_persist_enabled=true/false 时 Java apply 与 Rust persist 的行为矩阵。
  3. 定义哪些模式允许双写，哪些模式禁止双写。
  4. 明确 SHADOW 模式的安全约束，禁止污染 canonical state。
  5. 把结论写进文档，并补启动期保护逻辑设计。

  - Owner：A + JX + RX
  - 依赖：无
  - 产出：一页模式真值表、配置规则、启动保护规则
  - 完成标准：任何人都能明确回答“当前哪个路径才是权威写路径”

  ARCH-02 冻结 energy_limit 线协议语义

  1. 盘点 Java 当前发送值的来源与单位。
  2. 盘点 Rust 当前接收后的换算逻辑。
  3. 盘点 fixture/golden/replay 当前假设的单位。
  4. 选定唯一规范：传 SUN 或传 energy units。
  5. 定义兼容策略，避免老测试和新协议混用。

  - Owner：JX + RX
  - 依赖：无
  - 产出：协议说明、迁移策略
  - 完成标准：Java、Rust、fixture 三边单位定义一致

  ARCH-03 冻结 transaction/snapshot 语义合同

  1. 定义 storage transaction 是否承诺原子性、隔离性、跨 DB 能力。
  2. 定义 snapshot 是否必须是真时间点快照。
  3. 定义 execution 是否允许依赖 snapshot/revert。
  4. 定义短期如果做不到时的 fail-fast 策略。
  5. 输出接口状态表：支持、部分支持、禁用。

  - Owner：A + RS + JS
  - 依赖：无
  - 产出：storage 语义合同文档
  - 完成标准：不再存在“接口名支持但语义是假”的灰区

  ARCH-04 建 remote contract 支持矩阵

  1. 列出所有合约类型。
  2. 按 canonical remote / shadow only / Java only 分类。
  3. 对应到当前 feature flags。
  4. 对每个合约填上风险级别和验收门槛。
  5. 定义从 shadow only 升级到 canonical remote 的流程。

  - Owner：A + JX + RX + V
  - 依赖：ARCH-01
  - 产出：支持矩阵表
  - 完成标准：任何 remote 开关都有明确归属和准入标准

  Epic 2：Execution 读路径闭环

  EXEC-01 接通 Java callContract/estimateEnergy 到 Rust

  1. 梳理 Java RemoteExecutionSPI 当前 placeholder 分支。
  2. 对接 Rust 现有 call_contract RPC。
  3. 对接 Rust 现有 estimate_energy RPC。
  4. 对齐异常返回、超时、fallback 语义。
  5. 补 Java 单测和最小集成验证。

  - Owner：JX
  - 依赖：ARCH-02
  - 产出：Java bridge 真正调用 Rust
  - 完成标准：不再返回占位结果，SHADOW 能比较真实结果

  EXEC-02 实现 Rust getCode/getStorageAt/getNonce/getBalance

  1. 明确四个查询的底层存储来源。
  2. 补 gRPC server 端实现。
  3. 校验地址、key、空值、未找到语义。
  4. 回填 Java 侧调用路径。
  5. 增加查询类测试样例。

  - Owner：RX
  - 依赖：ARCH-01 + ARCH-03
  - 产出：execution query API 最小闭环
  - 完成标准：对 EOA 与已部署合约能返回可信结果

  EXEC-03 处理 execution snapshot/revert/health

  1. 决定 snapshot/revert 是做真语义还是 fail-fast。
  2. 如果做真语义，设计与 storage snapshot 的映射关系。
  3. 如果不做，Java 侧显式返回 unsupported，不伪成功。
  4. healthCheck 真实连接 backend，输出有意义状态。
  5. 增加异常场景测试。

  - Owner：RX + JX
  - 依赖：ARCH-03
  - 产出：snapshot/health 的真实语义
  - 完成标准：不再存在“快照伪成功”“health 恒异常”的占位状态

  Epic 3：Storage 语义完整性

  STOR-01 贯通 transaction_id 到写路径

  1. 确认 proto 字段使用规则。
  2. 改 Java RemoteStorageSPI 的 put/delete/batchWrite 以携带 transaction_id。
  3. 改 Rust gRPC server 识别 transaction_id。
  4. 明确没有 transaction_id 时的默认行为。
  5. 增加链路日志与诊断字段。

  - Owner：JS + RS
  - 依赖：ARCH-03
  - 产出：transaction_id 端到端贯通
  - 完成标准：事务上下文内的写不再直接落盘

  STOR-02 实现事务写缓冲与 commit/rollback

  1. 给 engine 增加真实 transaction write buffer。
  2. 让 put/delete/batchWrite 在事务内进入 buffer。
  3. commit 时一次性刷写。
  4. rollback 时丢弃 buffer。
  5. 补事务单测与并发边界测试。

  - Owner：RS
  - 依赖：STOR-01
  - 产出：真实事务能力
  - 完成标准：commit 可见、rollback 不可见、无事务时行为不变

  STOR-03 实现真 snapshot 或显式禁用

  1. 评估 RocksDB snapshot 方案。
  2. 如果实现，接入 snapshot 生命周期管理。
  3. 如果不实现，接口统一 fail-fast。
  4. 调整 Java 侧与 execution 侧调用预期。
  5. 补 snapshot 测试。

  - Owner：RS
  - 依赖：ARCH-03
  - 产出：snapshot 真能力或明确禁用
  - 完成标准：不再从 snapshot 读取当前库状态

  STOR-04 建 storage 测试基线

  1. 为 storage crate 建基础单测框架。
  2. 覆盖 CRUD、batch、iterator。
  3. 覆盖 transaction。
  4. 覆盖 snapshot。
  5. 覆盖异常路径与资源释放。

  - Owner：RS + V
  - 依赖：STOR-02 + STOR-03
  - 产出：storage 测试矩阵
  - 完成标准：cargo test -p tron-backend-storage 不再是 0 tests

  Epic 4：执行边角语义收口

  PAR-01 补齐 TriggerSmartContract 的 TRC-10 pre-transfer

  1. 复盘 Java VMActuator.call() 语义。
  2. 设计 Rust 侧 pre-exec token transfer 流程。
  3. 明确与失败回滚、energy、事件、副作用的关系。
  4. 实现并补 conformance case。
  5. 放入 contract 支持矩阵重新评级。

  - Owner：RX
  - 依赖：ARCH-02 + STOR-01
  - 产出：TRC-10 与 VM 交叉路径补齐
  - 完成标准：tokenValue > 0 的 trigger 不再被硬拒绝且状态一致

  PAR-02 收敛资源/手续费/energy parity

  1. 列出当前所有 sidecar：freeze、resource、vote、withdraw、TRC-10。
  2. 对每类 sidecar 建 Java vs Rust 对照样例。
  3. 收敛 energy_used 计算差异。
  4. 收敛 bandwidth/fee/global resource totals 差异。
  5. 输出 contract-type parity 报告。

  - Owner：RX + JX + V
  - 依赖：EXEC-01 + EXEC-02
  - 产出：资源/费用语义对照表
  - 完成标准：白名单合约在关键指标上进入可接受偏差

  PAR-03 收敛 feature flags 与桥接件退出计划

  1. 盘点所有 execution.remote.* 开关。
  2. 盘点 ResourceSyncService 等桥接件的职责。
  3. 标记哪些是临时补丁，哪些是长期接口。
  4. 定义默认配置集与实验配置集。
  5. 制定桥接件删除顺序。

  - Owner：A + JX + RX
  - 依赖：ARCH-04
  - 产出：开关分层与退出计划
  - 完成标准：配置默认值、文档、矩阵三者一致

  Epic 5：验证体系

  VER-01 让 golden vectors 真正执行 remote/shadow

  1. 梳理现有 golden vector 数据集。
  2. 接到真实 execute/call 路径。
  3. 定义结果比较维度：success、energy、return data、state changes。
  4. 增加 mismatch 输出格式。
  5. 把高价值 vector 编入日常回归。

  - Owner：V + JX
  - 依赖：EXEC-01 + EXEC-02
  - 产出：真实 golden verification
  - 完成标准：golden suite 能发现 remote/shadow 真实差异

  VER-02 建历史区块 replay 最小闭环

  1. 选固定 block range。
  2. 打通 replay 的加载、执行、记录流程。
  3. 输出 block、tx、mismatch 统计。
  4. 加上按 contract type 分类。
  5. 形成可复跑的报告模板。

  - Owner：V + JX + RX
  - 依赖：EXEC-01 + PAR-02
  - 产出：历史回放报告
  - 完成标准：每次回放都能稳定产出可比较指标

  VER-03 建 contract-type 准入看板

  1. 以 ARCH-04 的矩阵为基础建表。
  2. 每类 contract 绑定 golden/replay/test 覆盖情况。
  3. 记录当前状态：未测、shadow 稳定、canonical-ready、禁止 remote。
  4. 每周更新一次状态。
  5. 作为开关开启前置条件。

  - Owner：A + V
  - 依赖：VER-01 + VER-02
  - 产出：准入看板
  - 完成标准：任何 remote 开关开启前都能引用这张表

  VER-04 建 CI remote/shadow smoke gate

  1. 选一组最小 remote storage smoke。
  2. 选一组最小 remote execution smoke。
  3. 选一组 shadow mismatch smoke。
  4. 接入 CI。
  5. 对失败结果做可读输出。

  - Owner：V
  - 依赖：STOR-04 + VER-01
  - 产出：CI 门禁
  - 完成标准：PR 能看到 remote/shadow 是否回归

  Epic 6：为下一阶段做准备，但先不做 P2P

  BLOCK-01 设计 Rust Block Importer 目标边界

  1. 拆解 Java Manager.processBlock 的职责。
  2. 区分必须迁移到 Rust 的职责与可暂留 Java 的职责。
  3. 定义 block importer 输入输出模型。
  4. 定义与 consensus/maintenance 的边界。
  5. 输出下一阶段 ADR。

  - Owner：A + RX + JX
  - 依赖：Phase 1 出口条件基本满足
  - 产出：Block Importer 设计说明
  - 完成标准：下一阶段不是泛泛说“做区块同步”，而是有清晰 importer 边界

  ———

  建议分人

  - A：ARCH-01/03/04 PAR-03 BLOCK-01
  - JX：ARCH-02 EXEC-01 EXEC-03 VER-01
  - JS：STOR-01
  - RX：EXEC-02 PAR-01 PAR-02
  - RS：STOR-02/03/04
  - V：VER-02/03/04

  ———

  建议 Sprint 切法

  Sprint 1

  - ARCH-01
  - ARCH-02
  - ARCH-03
  - ARCH-04
  - EXEC-01
  - STOR-01

  目标：

  - 所有灰区语义冻结
  - Java 不再在 call/estimate 上是假实现
  - transaction_id 链路打通

  Sprint 2

  - EXEC-02
  - EXEC-03
  - STOR-02
  - STOR-03

  目标：

  - execution 读路径闭环
  - storage 事务/快照不再名不副实

  Sprint 3

  - STOR-04
  - PAR-01
  - PAR-02
  - PAR-03

  目标：

  - 补最危险的执行边角
  - 把 rollout 从“实验性可跑”变成“可判定稳定性”

  Sprint 4

  - VER-01
  - VER-02
  - VER-03
  - VER-04

  目标：

  - 建立准入机制
  - 开始为 BLOCK-01 做前置判断

  ———

  我建议你真正先开的 Top 8 issues
  如果你现在只开第一批，不要一下子开太多，我建议先开这 8 个：

  1. ARCH-01
  2. ARCH-02
  3. ARCH-03
  4. ARCH-04
  5. EXEC-01
  6. STOR-01
  7. EXEC-02
  8. STOR-02

  这是因为这 8 个解决的是“现在有没有资格继续往下迁”的问题。

  ———

  Phase 1 的退出标准
  你什么时候可以说“storage 和 smart contract 不再只是基本实现，而是进入下一阶段”：

  1. call/estimate/query 全部不是 placeholder。
  2. storage transaction/snapshot 语义真实，或被明确禁用。
  3. energy_limit 规范冻结，配置默认值和代码默认值一致。
  4. rust_persist_enabled 的角色清晰，不再处于危险混用状态。
  5. 至少一批核心 contract type 达到 shadow stable。
  6. storage crate 有真实测试，不再是 0 tests。
  7. replay/CI 能稳定告诉你当前 parity 水位。

  做到这一步，你下一步就该开 BLOCK-01 Rust Block Importer，而不是 P2P-01。