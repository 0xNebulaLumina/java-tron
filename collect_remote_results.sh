#!/bin/bash

set -e

# Configurable sleep duration (in seconds), default 1200 (20 minutes)
SLEEP_DURATION=${1:-1200}
# Configurable embedded Java log path
EMBEDDED_JAVA_LOG=${2:-1.embedded-java.log}
# Configurable embedded-embedded CSV path
EMBEDDED_CSV=${3:-output-directory/execution-csv/20251215-021928-e76f9825-embedded-embedded.csv}

echo "Starting remote execution + remote storage result collection..."
echo "Sleep duration: ${SLEEP_DURATION} seconds ($(($SLEEP_DURATION / 60)) minutes)"
echo "Embedded Java log: ${EMBEDDED_JAVA_LOG}"
echo "Embedded CSV: ${EMBEDDED_CSV}"

# Step 1: Clean up previous data
echo "Step 1: Cleaning up previous data..."
rm -rf data/ logs/ output-directory/database/ rust-backend/data/ framework/data/ framework/logs/ execution-reports/ *.log

# Step 2: Compile rust-backend
echo "Step 2: Compiling rust-backend..."
cd rust-backend
cargo build --release
cd ..

# Step 3: Compile java-tron
echo "Step 3: Compiling java-tron..."
./gradlew clean build -x test --dependency-verification=off

# Step 4: Start rust-backend in background
echo "Step 4: Starting rust-backend..."
cd rust-backend
nohup ./target/release/tron-backend >> ../rust.log 2>&1 &
RUST_PID=$!
cd ..

# Wait for rust-backend to initialize
echo "Waiting 30 seconds for rust-backend to initialize..."
sleep 30

# Step 5: Start java-tron
echo "Step 5: Starting java-tron..."
nohup java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly  -XX:CMSInitiatingOccupancyFraction=70 \
     -Dexec.csv.enabled=true -Dexec.csv.stateChanges.enabled=true \
     -Dremote.exec.trc10.enabled=true -Dremote.exec.apply.trc10=false \
     -Dremote.resource.sync.debug=true -Dremote.resource.sync.confirm=true \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "REMOTE" >> start.log 2>&1 &
JAVA_PID=$!

echo "Java-tron started with PID: $JAVA_PID"
echo "Rust-backend started with PID: $RUST_PID"

# Step 6: Wait for configured duration then stop services
echo "Current time: $(date '+%Y-%m-%d %H:%M:%S')"
echo "Waiting ${SLEEP_DURATION} seconds for data collection..."
sleep $SLEEP_DURATION

echo "Step 6: Stopping services..."
# Stop java-tron first
if kill -0 $JAVA_PID 2>/dev/null; then
    echo "Stopping java-tron (PID: $JAVA_PID)..."
    kill $JAVA_PID
    sleep 10
    # Force kill if still running
    if kill -0 $JAVA_PID 2>/dev/null; then
        kill -9 $JAVA_PID
    fi
fi

# Stop rust-backend
if kill -0 $RUST_PID 2>/dev/null; then
    echo "Stopping rust-backend (PID: $RUST_PID)..."
    kill $RUST_PID
    sleep 5
    # Force kill if still running
    if kill -0 $RUST_PID 2>/dev/null; then
        kill -9 $RUST_PID
    fi
fi

# Step 7: Get git commit hash
echo "Step 7: Getting git commit hash..."
GIT_COMMIT_HASH=$(git rev-parse --short=7 HEAD)
echo "Current commit hash: $GIT_COMMIT_HASH"

# Step 8: Move Java logs
echo "Step 8: Moving Java logs..."
if [ -f "logs/tron.log" ]; then
    mv logs/tron.log "remote-java.${GIT_COMMIT_HASH}.log"
    JAVA_LOG_PATH="remote-java.${GIT_COMMIT_HASH}.log"
    echo "Java log moved to: $JAVA_LOG_PATH"
else
    echo "Warning: logs/tron.log not found"
    JAVA_LOG_PATH="logs/tron.log (not found)"
fi

# Step 9: Move Rust logs
echo "Step 9: Moving Rust logs..."
if [ -f "rust.log" ]; then
    mv rust.log "remote-rust.${GIT_COMMIT_HASH}.log"
    RUST_LOG_PATH="remote-rust.${GIT_COMMIT_HASH}.log"
    echo "Rust log moved to: $RUST_LOG_PATH"
else
    echo "Warning: rust.log not found"
    RUST_LOG_PATH="rust.log (not found)"
fi

cp "../archive/${EMBEDDED_JAVA_LOG}" ./

# Step 10: Find newest CSV file
echo "Step 10: Finding newest CSV file..."
if [ -d "output-directory/execution-csv/" ]; then
    NEWEST_CSV=$(find output-directory/execution-csv/ -name "*.csv" -type f -printf '%T@ %p\n' | sort -n | tail -1 | cut -d' ' -f2-)
    if [ -n "$NEWEST_CSV" ]; then
        echo "Newest CSV found: $NEWEST_CSV"
    else
        echo "Warning: No CSV files found in output-directory/execution-csv/"
        NEWEST_CSV="output-directory/execution-csv/ (no CSV files found)"
    fi
else
    echo "Warning: output-directory/execution-csv/ directory not found"
    NEWEST_CSV="output-directory/execution-csv/ (directory not found)"
fi

echo ""
echo "============================================"
echo "COLLECTION COMPLETE - FILE PATHS:"
echo "============================================"
echo "10.1 Remote Java log: $JAVA_LOG_PATH"
echo "10.2 Remote Rust log: $RUST_LOG_PATH"
echo "10.3 Newest CSV file: $NEWEST_CSV"
echo "============================================"
echo ""

# Step 11: Run CSV comparison
echo "Step 11: Running CSV comparison..."
python3 scripts/compare_exec_csv.py "$EMBEDDED_CSV" "$NEWEST_CSV"

echo ""
echo "’‘’"
echo "Think harder."
echo ""
echo "I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,"
echo ""
echo "The result csv are"
echo "+ $EMBEDDED_CSV"
echo "+ $NEWEST_CSV"
echo "respectively."
echo ""
python3 scripts/compare_exec_csv.py "$EMBEDDED_CSV" "$NEWEST_CSV"
echo ""
echo "Logs:"
echo "(embedded execution + embedded storage) java log: $EMBEDDED_JAVA_LOG"
echo "(remote execution + remote storage) java log: $JAVA_LOG_PATH"
echo "(remote execution + remote storage) rust log: $RUST_LOG_PATH"
echo ""
echo "You will help me debug and figure out why there are mismatches."
echo "’‘’"
