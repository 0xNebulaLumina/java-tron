# Tron Workload Testing

本文档介绍如何使用新的生产级 Tron 工作负载测试来验证存储层在真实区块链场景下的性能。

## 概述

Tron 工作负载测试模拟了真实的 java-tron 区块链操作模式，包括：

1. **区块处理工作负载** - 模拟区块生产和交易处理
2. **账户查询工作负载** - 模拟钱包和浏览器的账户查询
3. **交易历史工作负载** - 模拟区块链浏览器的历史数据查询
4. **智能合约状态工作负载** - 模拟智能合约的状态读写
5. **快速同步工作负载** - 模拟节点快速同步的批量数据操作
6. **混合负载压力测试** - 模拟生产环境的并发混合操作

## 快速开始

### 运行完整的 Tron 工作负载测试

```bash
# 运行所有 Tron 工作负载测试（包括嵌入式和远程存储）
make tron-workload-test

# 或者直接使用脚本
./scripts/run-performance-tests.sh
```

### 运行特定存储模式的测试

```bash
# 仅测试嵌入式存储的 Tron 工作负载
make embedded-tron-workload

# 仅测试远程存储的 Tron 工作负载（需要启动 gRPC 服务）
make remote-tron-workload
```

### 运行特定的工作负载场景

```bash
# 区块处理测试
make tron-block-processing

# 账户查询测试
make tron-account-query

# 交易历史测试
make tron-tx-history

# 智能合约状态测试
make tron-contract-state

# 快速同步测试
make tron-fast-sync

# 混合负载压力测试
make tron-stress-test
```

## 测试场景详解

### 1. 区块处理工作负载 (Block Processing Workload)

**测试目标**: 验证存储层是否能够支持主网级别的区块处理性能

**测试内容**:
- 处理 100 个区块，每个区块包含 2000 笔交易
- 模拟区块头存储、交易批量写入、账户状态更新、动态属性更新
- 测试顺序写入性能和批量操作吞吐量

**性能目标**:
- 交易吞吐量 ≥ 1000 TPS（主网 50% 性能要求）
- 区块处理延迟 < 3 秒（满足 3 秒出块要求）

**关键指标**:
- `transaction_throughput`: 交易处理吞吐量 (tx/sec)
- `block_throughput`: 区块处理吞吐量 (blocks/sec)
- `avg_block_latency`: 平均区块处理延迟 (ms)

### 2. 账户查询工作负载 (Account Query Workload)

**测试目标**: 验证高频账户查询的响应性能

**测试内容**:
- 创建 10,000 个测试账户
- 执行 50,000 次随机账户查询
- 模拟钱包余额查询和区块链浏览器访问模式

**性能目标**:
- 平均查询延迟 < 50ms（95th percentile）
- 查询吞吐量 > 1000 queries/sec

**关键指标**:
- `avg_query_latency`: 平均查询延迟 (ms)
- `query_throughput`: 查询吞吐量 (queries/sec)
- `min_query_latency` / `max_query_latency`: 延迟范围

### 3. 交易历史工作负载 (Transaction History Workload)

**测试目标**: 验证历史数据查询和检索性能

**测试内容**:
- 存储 100,000 笔历史交易
- 执行 1,000 次随机交易哈希查询
- 模拟区块链浏览器的交易查询场景

**性能目标**:
- 查询成功率 = 100%
- 平均查询延迟 < 100ms

**关键指标**:
- `success_rate`: 查询成功率 (%)
- `avg_query_latency`: 平均查询延迟 (ms)
- `query_throughput`: 查询吞吐量 (queries/sec)

### 4. 智能合约状态工作负载 (Smart Contract State Workload)

**测试目标**: 验证智能合约状态的读写性能

**测试内容**:
- 模拟 1,000 个智能合约
- 每个合约执行 100 次状态读写操作
- 测试小数据块（64 字节）的高频更新

**性能目标**:
- 写入延迟 < 10ms
- 读取延迟 < 5ms

**关键指标**:
- `avg_write_latency`: 平均写入延迟 (ms)
- `avg_read_latency`: 平均读取延迟 (ms)
- `operation_throughput`: 操作吞吐量 (ops/sec)

### 5. 快速同步工作负载 (Fast Sync Workload)

**测试目标**: 验证批量数据同步的吞吐量性能

**测试内容**:
- 100 个批次，每批次 10,000 条记录
- 总计 1,000,000 条记录的批量写入
- 模拟节点快速同步场景

**性能目标**:
- 数据吞吐量 ≥ 10 MB/sec
- 支持大批量操作

**关键指标**:
- `sync_throughput`: 同步吞吐量 (ops/sec)
- `data_throughput`: 数据吞吐量 (MB/sec)
- `avg_batch_latency`: 平均批次延迟 (ms)

### 6. 混合负载压力测试 (Mixed Workload Stress Test)

**测试目标**: 验证并发混合操作下的系统稳定性

**测试内容**:
- 10 个并发线程，运行 60 秒
- 混合操作：70% 读取，20% 写入，10% 批量操作
- 模拟生产环境的真实负载模式

**性能目标**:
- 并发吞吐量 ≥ 1000 ops/sec
- 系统稳定运行，无崩溃

**关键指标**:
- `stress_throughput`: 压力测试吞吐量 (ops/sec)
- `avg_latency`: 平均操作延迟 (ms)
- `total_operations`: 总操作数

## 测试报告和分析

### 报告文件位置

测试完成后，详细报告保存在 `reports/` 目录下：

```
reports/YYYYMMDD-HHMMSS/
├── embedded-tron-*.log          # 嵌入式存储测试日志
├── remote-tron-*.log            # 远程存储测试日志
├── extracted-metrics.txt        # 提取的性能指标
├── extracted-metrics.csv        # CSV 格式的指标数据
└── performance-summary.md       # 性能总结报告
```

### 关键性能指标

| 指标类别 | 指标名称 | 目标值 | 说明 |
|---------|---------|--------|------|
| 区块处理 | transaction_throughput | ≥1000 TPS | 交易处理吞吐量 |
| 账户查询 | avg_query_latency | <50ms | 平均查询延迟 |
| 交易历史 | success_rate | 100% | 查询成功率 |
| 合约状态 | avg_write_latency | <10ms | 写入延迟 |
| 合约状态 | avg_read_latency | <5ms | 读取延迟 |
| 快速同步 | data_throughput | ≥10 MB/sec | 数据吞吐量 |
| 压力测试 | stress_throughput | ≥1000 ops/sec | 并发吞吐量 |

### 性能对比分析

测试框架会自动生成嵌入式存储和远程存储的性能对比：

```bash
# 查看性能对比报告
cat reports/*/performance-summary.md

# 分析性能指标
grep "METRIC:" reports/*/extracted-metrics.txt
```

## 故障排查

### 常见问题

1. **gRPC 连接失败**
   ```bash
   # 确保 Rust 存储服务正在运行
   make rust-run
   
   # 检查服务健康状态
   curl -s http://localhost:50011 || echo "Service not responding"
   ```

2. **内存不足**
   ```bash
   # 增加 JVM 堆内存
   export GRADLE_OPTS="-Xmx4g"
   ```

3. **测试超时**
   ```bash
   # 检查系统资源使用情况
   top
   iostat -x 1
   ```

### 日志分析

```bash
# 查看详细的测试执行日志
tail -f reports/*/remote-tron-*.log

# 提取性能指标
grep "METRIC:" reports/*/remote-tron-*.log
```

## 定制化测试

### 修改测试参数

编辑 `TronWorkloadBenchmark.java` 中的常量：

```java
// Tron-specific constants
private static final int TRON_BLOCK_SIZE = 2000; // 每个区块的交易数
private static final int TRON_MAINNET_TPS = 2000; // 目标 TPS
private static final int TRON_SYNC_BATCH_SIZE = 10000; // 同步批次大小
```

### 添加新的测试场景

1. 在 `TronWorkloadBenchmark.java` 中添加新的 `@Test` 方法
2. 实现具体的测试逻辑
3. 添加性能指标收集
4. 更新 `run-performance-tests.sh` 脚本

## 最佳实践

1. **测试环境准备**
   - 使用 SSD 存储以获得准确的性能数据
   - 确保足够的内存（建议 8GB+）
   - 关闭其他资源密集型应用

2. **性能基线建立**
   - 首先运行嵌入式存储测试建立基线
   - 然后运行远程存储测试进行对比
   - 多次运行取平均值以确保结果稳定

3. **结果分析**
   - 关注平均延迟和 95th percentile 延迟
   - 分析吞吐量是否满足业务需求
   - 检查资源使用情况（CPU、内存、磁盘 I/O）

4. **持续监控**
   - 定期运行测试以监控性能回归
   - 在代码变更后运行测试验证影响
   - 建立性能监控和告警机制 