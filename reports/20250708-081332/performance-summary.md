# Performance Testing Summary

**Test Date:** Tue Jul  8 08:20:23 AM CEST 2025
**gRPC Server:** localhost:50011
**Java Version:** openjdk version "1.8.0_452"
**System Info:** Linux kvm12956.bero-host.de 6.8.0-63-generic #66-Ubuntu SMP PREEMPT_DYNAMIC Fri Jun 13 20:25:30 UTC 2025 x86_64 x86_64 x86_64 GNU/Linux

## Test Results

### Unit Tests
- ✅ Basic StorageSPI functionality tests passed

### Integration Tests  
- ✅ End-to-end gRPC communication tests passed
- ✅ All CRUD operations working correctly
- ✅ Batch operations functioning properly
- ✅ Transaction and snapshot support verified

### Performance Benchmarks
- ✅ Single operation latency measured
- ✅ Batch operation throughput tested
- ✅ System resource usage analyzed

## Detailed Reports
- [embedded-benchmarkBatchOperationThroughput.log](embedded-benchmarkBatchOperationThroughput.log)
- [embedded-benchmarkSingleOperationLatency.log](embedded-benchmarkSingleOperationLatency.log)
- [embedded-generatePerformanceReport.log](embedded-generatePerformanceReport.log)
- [embedded-tron-benchmarkAccountQueryWorkload.log](embedded-tron-benchmarkAccountQueryWorkload.log)
- [embedded-tron-benchmarkBlockProcessingWorkload.log](embedded-tron-benchmarkBlockProcessingWorkload.log)
- [embedded-tron-benchmarkFastSyncWorkload.log](embedded-tron-benchmarkFastSyncWorkload.log)
- [embedded-tron-benchmarkMixedWorkloadStressTest.log](embedded-tron-benchmarkMixedWorkloadStressTest.log)
- [embedded-tron-benchmarkSmartContractStateWorkload.log](embedded-tron-benchmarkSmartContractStateWorkload.log)
- [embedded-tron-benchmarkTransactionHistoryWorkload.log](embedded-tron-benchmarkTransactionHistoryWorkload.log)
- [remote-benchmarkBatchOperationThroughput.log](remote-benchmarkBatchOperationThroughput.log)
- [remote-benchmarkSingleOperationLatency.log](remote-benchmarkSingleOperationLatency.log)
- [remote-generatePerformanceReport.log](remote-generatePerformanceReport.log)
- [remote-tron-benchmarkAccountQueryWorkload.log](remote-tron-benchmarkAccountQueryWorkload.log)
- [remote-tron-benchmarkBlockProcessingWorkload.log](remote-tron-benchmarkBlockProcessingWorkload.log)
- [remote-tron-benchmarkFastSyncWorkload.log](remote-tron-benchmarkFastSyncWorkload.log)
- [remote-tron-benchmarkMixedWorkloadStressTest.log](remote-tron-benchmarkMixedWorkloadStressTest.log)
- [remote-tron-benchmarkSmartContractStateWorkload.log](remote-tron-benchmarkSmartContractStateWorkload.log)
- [remote-tron-benchmarkTransactionHistoryWorkload.log](remote-tron-benchmarkTransactionHistoryWorkload.log)
- [Extracted Metrics Summary](extracted-metrics.txt)
- [Metrics CSV Data](extracted-metrics.csv)

## Key Performance Metrics

```
# Extracted Performance Metrics
Timestamp: Tue Jul  8 08:20:23 AM CEST 2025

# EMBEDDED STORAGE TESTS

## benchmarkBatchOperationThroughput (Embedded)

  batch_write_10_latency: 4.333696 ms
  batch_write_10_throughput: 2307.499188 ops/sec
  batch_write_10_bandwidth: 0.563354 MB/sec
  batch_get_10_latency: 0.675758 ms
  batch_get_10_throughput: 14798.196988 ops/sec
  batch_get_10_bandwidth: 3.612841 MB/sec
  batch_write_50_latency: 0.279348 ms
  batch_write_50_throughput: 178988.215416 ops/sec
  batch_write_50_bandwidth: 43.698295 MB/sec
  batch_get_50_latency: 0.292415 ms
  batch_get_50_throughput: 170989.860301 ops/sec
  batch_get_50_bandwidth: 41.745571 MB/sec
  batch_write_100_latency: 0.461146 ms
  batch_write_100_throughput: 216851.062353 ops/sec
  batch_write_100_bandwidth: 52.942154 MB/sec
  batch_get_100_latency: 0.473311 ms
  batch_get_100_throughput: 211277.574364 ops/sec
  batch_get_100_bandwidth: 51.581439 MB/sec
  batch_write_500_latency: 1.419209 ms
  batch_write_500_throughput: 352308.927015 ops/sec
  batch_write_500_bandwidth: 86.012922 MB/sec
  batch_get_500_latency: 1.592950 ms
  batch_get_500_throughput: 313883.047177 ops/sec
  batch_get_500_bandwidth: 76.631603 MB/sec
  batch_write_1000_latency: 1.961164 ms
  batch_write_1000_throughput: 509901.262719 ops/sec
  batch_write_1000_bandwidth: 124.487613 MB/sec
  batch_get_1000_latency: 1.482623 ms
  batch_get_1000_throughput: 674480.296070 ops/sec
  batch_get_1000_bandwidth: 164.668041 MB/sec
      BENCHMARK: EmbeddedBatchOperationThroughput
      METRIC: EmbeddedBatchOperationThroughput.batch_write_10_latency = 4.333696 ms
      METRIC: EmbeddedBatchOperationThroughput.batch_write_10_throughput = 2307.499188 ops/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_write_10_bandwidth = 0.563354 MB/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_get_10_latency = 0.675758 ms
      METRIC: EmbeddedBatchOperationThroughput.batch_get_10_throughput = 14798.196988 ops/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_get_10_bandwidth = 3.612841 MB/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_write_50_latency = 0.279348 ms
      METRIC: EmbeddedBatchOperationThroughput.batch_write_50_throughput = 178988.215416 ops/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_write_50_bandwidth = 43.698295 MB/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_get_50_latency = 0.292415 ms
      METRIC: EmbeddedBatchOperationThroughput.batch_get_50_throughput = 170989.860301 ops/sec
      METRIC: EmbeddedBatchOperationThroughput.batch_get_50_bandwidth = 41.745571 MB/sec
```

## Next Steps
1. Compare results with embedded storage baseline
2. Optimize performance bottlenecks if identified
3. Run load tests with production-like workloads
4. Validate performance under concurrent access patterns

## Recommendations
- Monitor latency trends over extended periods
- Test with various data sizes and access patterns
- Validate performance under network stress conditions
- Consider connection pooling optimizations if needed
