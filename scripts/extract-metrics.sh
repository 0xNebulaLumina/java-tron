#!/bin/bash

# Simple metrics extraction script for performance test logs
# Usage: ./extract-metrics.sh <log-file>

set -e

if [ $# -eq 0 ]; then
    echo "Usage: $0 <log-file>"
    exit 1
fi

LOG_FILE="$1"

if [ ! -f "$LOG_FILE" ]; then
    echo "Error: Log file '$LOG_FILE' not found"
    exit 1
fi

REPORT_DIR=$(dirname "$LOG_FILE")
METRICS_FILE="$REPORT_DIR/performance-metrics-summary.txt"

echo "Extracting metrics from: $LOG_FILE"
echo "Output: $METRICS_FILE"

# Create metrics summary
cat > "$METRICS_FILE" << EOF
# Performance Metrics Summary
Generated at: $(date)
Source: $(basename "$LOG_FILE")

EOF

# Extract METRIC: lines
echo "## Key Performance Metrics" >> "$METRICS_FILE"
echo "" >> "$METRICS_FILE"

if grep -q "METRIC:" "$LOG_FILE"; then
    grep "METRIC:" "$LOG_FILE" | sed 's/METRIC: //' | sort >> "$METRICS_FILE"
    echo "" >> "$METRICS_FILE"
else
    echo "No structured metrics found in log file" >> "$METRICS_FILE"
    echo "" >> "$METRICS_FILE"
fi

# Extract benchmark summaries
echo "## Benchmark Summaries" >> "$METRICS_FILE"
echo "" >> "$METRICS_FILE"

# Look for test headers and summaries
grep -A 10 "BENCHMARK:" "$LOG_FILE" | grep -E "(BENCHMARK:|Average|Throughput|ops/sec|ms)" >> "$METRICS_FILE" 2>/dev/null || echo "No benchmark summaries found" >> "$METRICS_FILE"

echo "" >> "$METRICS_FILE"

# Extract any performance-related output
echo "## Additional Performance Data" >> "$METRICS_FILE"
echo "" >> "$METRICS_FILE"

grep -E "(latency|throughput|ops/sec|MB/sec|ms)" "$LOG_FILE" | grep -v "METRIC:" | head -20 >> "$METRICS_FILE" 2>/dev/null || echo "No additional performance data found" >> "$METRICS_FILE"

echo "Metrics extraction completed: $METRICS_FILE"

# Also create a simple CSV if we found structured metrics
CSV_FILE="$REPORT_DIR/performance-metrics-summary.csv"
if grep -q "METRIC:" "$LOG_FILE"; then
    echo "TestName,MetricName,Value,Unit" > "$CSV_FILE"
    grep "METRIC:" "$LOG_FILE" | sed 's/METRIC: //' | while IFS= read -r line; do
        if [[ $line =~ ([^.]+)\.([^\ ]+)\ =\ ([0-9.]+)\ (.+) ]]; then
            echo "${BASH_REMATCH[1]},${BASH_REMATCH[2]},${BASH_REMATCH[3]},${BASH_REMATCH[4]}" >> "$CSV_FILE"
        fi
    done
    echo "CSV metrics file created: $CSV_FILE"
fi 