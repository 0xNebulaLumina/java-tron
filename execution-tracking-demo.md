# Execution Tracking Demo

This document demonstrates how to use the new execution tracking system to compare behavior between embedded (Java) and remote (Rust) execution modes.

## Quick Start

### 1. Enable Execution Tracking

Add these configuration options to your `main_net_config_remote.conf`:

```hocon
execution {
  mode = "REMOTE"  # or "EMBEDDED"
  tracking {
    enabled = true
    output.dir = "./execution-metrics"
    state.digest = true
  }
}
```

Alternatively, use environment variables:
```bash
export EXECUTION_TRACKING_ENABLED=true
export EXECUTION_TRACKING_OUTPUT_DIR=./execution-metrics
export EXECUTION_TRACKING_STATE_DIGEST=true
```

Or system properties:
```bash
java -Dexecution.tracking.enabled=true \
     -Dexecution.tracking.output.dir=./execution-metrics \
     -Dexecution.tracking.state.digest=true \
     -jar FullNode.jar
```

### 2. Run with Different Execution Modes

#### Test Embedded Mode (Java EVM)
```bash
# Terminal 1: Run with embedded mode
export EXECUTION_MODE=EMBEDDED
export EXECUTION_TRACKING_ENABLED=true
export EXECUTION_TRACKING_OUTPUT_DIR=./metrics-embedded

java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly -XX:CMSInitiatingOccupancyFraction=70 \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf
```

#### Test Remote Mode (Rust Execution Service)
```bash
# Terminal 1: Start Rust backend service
cd rust-backend && cargo run --release

# Terminal 2: Run with remote mode  
export EXECUTION_MODE=REMOTE
export EXECUTION_TRACKING_ENABLED=true
export EXECUTION_TRACKING_OUTPUT_DIR=./metrics-remote

java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly -XX:CMSInitiatingOccupancyFraction=70 \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "REMOTE"
```

#### Test Shadow Mode (Both Engines with Comparison)
```bash
# Terminal 1: Start Rust backend service
cd rust-backend && cargo run --release

# Terminal 2: Run with shadow mode
export EXECUTION_MODE=SHADOW
export EXECUTION_TRACKING_ENABLED=true
export EXECUTION_TRACKING_OUTPUT_DIR=./metrics-shadow

java -Xms9G -Xmx9G [...same JVM args...] \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "SHADOW"
```

### 3. Analyze Results

The tracking system will create CSV files in the specified output directory:

```
./execution-metrics/
├── execution-metrics-2025-01-15.csv
├── execution-metrics-2025-01-16.csv
└── ...
```

#### CSV Schema
```csv
timestamp,tx_id,execution_mode,is_success,energy_used,return_data_hex,runtime_error,state_changes_count,block_number,block_timestamp,tx_type,state_digest,execution_time_ms
```

#### Sample CSV Data
```csv
timestamp,tx_id,execution_mode,is_success,energy_used,return_data_hex,runtime_error,state_changes_count,block_number,block_timestamp,tx_type,state_digest,execution_time_ms
2025-01-15T10:30:45.123Z,abc123...,EMBEDDED,true,21000,0x1234...,null,5,1000000,1642248645000,TriggerSmartContract,0xdeadbeef...,15
2025-01-15T10:30:45.456Z,abc123...,REMOTE,true,21000,0x1234...,null,5,1000000,1642248645000,TriggerSmartContract,0xdeadbeef...,12
```

## Analysis Examples

### Simple Comparison Script (Python)
```python
import pandas as pd

# Load CSV files
embedded_df = pd.read_csv('./metrics-embedded/execution-metrics-2025-01-15.csv')
remote_df = pd.read_csv('./metrics-remote/execution-metrics-2025-01-15.csv')

# Compare success rates
print(f"Embedded success rate: {embedded_df['is_success'].mean():.2%}")
print(f"Remote success rate: {remote_df['is_success'].mean():.2%}")

# Compare energy usage
print(f"Embedded avg energy: {embedded_df['energy_used'].mean():.0f}")
print(f"Remote avg energy: {remote_df['energy_used'].mean():.0f}")

# Compare execution times
print(f"Embedded avg time: {embedded_df['execution_time_ms'].mean():.1f}ms")
print(f"Remote avg time: {remote_df['execution_time_ms'].mean():.1f}ms")

# Find transactions with different state digests
if 'state_digest' in embedded_df.columns:
    merged = pd.merge(embedded_df, remote_df, on='tx_id', suffixes=('_embedded', '_remote'))
    different_digests = merged[merged['state_digest_embedded'] != merged['state_digest_remote']]
    print(f"Transactions with different state digests: {len(different_digests)}")
```

### Shell Script for Quick Stats
```bash
#!/bin/bash

echo "=== Execution Tracking Analysis ==="
echo

for dir in metrics-*/; do
    echo "Mode: ${dir#metrics-}"
    for csv in "$dir"*.csv; do
        if [[ -f "$csv" ]]; then
            total=$(tail -n +2 "$csv" | wc -l)
            successful=$(tail -n +2 "$csv" | cut -d',' -f4 | grep -c "true")
            avg_energy=$(tail -n +2 "$csv" | cut -d',' -f5 | awk '{sum+=$1} END {print sum/NR}')
            avg_time=$(tail -n +2 "$csv" | cut -d',' -f13 | awk '{sum+=$1} END {print sum/NR}')
            
            echo "  Total transactions: $total"
            echo "  Successful: $successful ($(( successful * 100 / total ))%)"
            echo "  Avg energy: ${avg_energy%.0f}"
            echo "  Avg time: ${avg_time%.0f}ms"
        fi
    done
    echo
done
```

## Configuration Reference

### Environment Variables
- `EXECUTION_TRACKING_ENABLED`: Enable/disable tracking (default: false)
- `EXECUTION_TRACKING_OUTPUT_DIR`: CSV output directory (default: ./execution-metrics)  
- `EXECUTION_TRACKING_STATE_DIGEST`: Compute state digest (default: true)

### System Properties
- `execution.tracking.enabled`: Enable/disable tracking
- `execution.tracking.output.dir`: CSV output directory
- `execution.tracking.state.digest`: Compute state digest

### Configuration File
```hocon
execution {
  tracking {
    enabled = true
    output.dir = "./execution-metrics"
    state.digest = true
  }
}
```

## Troubleshooting

### Common Issues

1. **No CSV files generated**
   - Check that tracking is enabled
   - Verify output directory permissions
   - Look for error messages in logs

2. **State digest errors**
   - Ensure StateDigest JNI library is available
   - Set `execution.tracking.state.digest = false` to disable if needed

3. **Performance impact**
   - Tracking adds minimal overhead (~1-2ms per transaction)
   - Use async CSV writing to minimize impact
   - Monitor metrics queue size via JMX if available

4. **Large CSV files**
   - Files rotate daily automatically
   - Consider archiving/compressing old files
   - Use head/tail commands for large file analysis

### Log Messages to Look For
```
INFO  ExecutionSpiFactory - Execution tracking is enabled, wrapping with TrackedExecutionSPI
INFO  ExecutionMetricsLogger - ExecutionMetricsLogger initialized. Output directory: ./execution-metrics
INFO  TrackedExecutionSPI - TrackedExecutionSPI initialized for mode: EMBEDDED, state digest: true
```

This tracking system provides comprehensive visibility into execution behavior differences between Java and Rust implementations during the migration process.