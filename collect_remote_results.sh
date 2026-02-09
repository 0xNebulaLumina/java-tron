#!/bin/bash

set -e

# Poll interval while waiting for java-tron to exit (seconds).
WAIT_INTERVAL=${WAIT_INTERVAL:-30}
# How often to print a progress line while waiting (seconds).
PROGRESS_INTERVAL=${PROGRESS_INTERVAL:-300}

# Configurable max wait duration (in seconds), default 1200 (20 minutes).
# If set to 0, wait until java-tron exits (e.g., via node.shutdown BlockHeight).
SLEEP_DURATION=${1:-1200}
# Configurable embedded Java log path
EMBEDDED_JAVA_LOG=${2:-621c89c.embedded-java.log}
# Configurable embedded-embedded CSV path
EMBEDDED_CSV=${3:-output-directory/execution-csv/20260128-131248-9835b834-embedded-embedded.csv}

# ResourceSync debug/confirm flags (default false). Override via env vars:
#   REMOTE_RESOURCE_SYNC_DEBUG=true
#   REMOTE_RESOURCE_SYNC_CONFIRM=true
REMOTE_RESOURCE_SYNC_DEBUG=${REMOTE_RESOURCE_SYNC_DEBUG:-false}
REMOTE_RESOURCE_SYNC_CONFIRM=${REMOTE_RESOURCE_SYNC_CONFIRM:-false}

JAVA_PID=""
RUST_PID=""

get_shutdown_height() {
    local conf_file=${1}
    if [ -f "${conf_file}" ]; then
        # Best-effort parse for: BlockHeight = 12345
        grep -E '^[[:space:]]*BlockHeight[[:space:]]*=' "${conf_file}" | head -n 1 | grep -Eo '[0-9]+' || true
    fi
}

get_java_head_block() {
    local log_file="logs/tron.log"
    if [ -f "${log_file}" ]; then
        tail -n 200 "${log_file}" \
            | grep -Eo 'Update latest block header number = [0-9]+' \
            | tail -n 1 \
            | awk '{print $NF}'
    fi
}

get_rust_block() {
    local log_file="rust.log"
    if [ -f "${log_file}" ]; then
        tail -n 400 "${log_file}" \
            | grep -Eo 'block: [0-9]+' \
            | tail -n 1 \
            | awk '{print $2}'
    fi
}

print_wait_status() {
    local java_head
    local rust_block
    java_head=$(get_java_head_block || true)
    rust_block=$(get_rust_block || true)

    local elapsed=${SECONDS}
    if [ "${SLEEP_DURATION}" -eq 0 ]; then
        if [ -n "${SHUTDOWN_HEIGHT}" ] && [ -n "${java_head}" ]; then
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Waiting... elapsed=${elapsed}s java_head=${java_head}/${SHUTDOWN_HEIGHT} rust_block=${rust_block:-?}"
        else
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Waiting... elapsed=${elapsed}s java_head=${java_head:-?} rust_block=${rust_block:-?}"
        fi
    else
        local remaining=$((end_time - SECONDS))
        if [ "${remaining}" -lt 0 ]; then
            remaining=0
        fi
        if [ -n "${SHUTDOWN_HEIGHT}" ] && [ -n "${java_head}" ]; then
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Waiting... elapsed=${elapsed}s remaining=${remaining}s java_head=${java_head}/${SHUTDOWN_HEIGHT} rust_block=${rust_block:-?}"
        else
            echo "[$(date '+%Y-%m-%d %H:%M:%S')] Waiting... elapsed=${elapsed}s remaining=${remaining}s java_head=${java_head:-?} rust_block=${rust_block:-?}"
        fi
    fi
}

stop_process() {
    local pid=${1}
    local name=${2}
    if [ -z "${pid}" ]; then
        return 0
    fi
    if kill -0 "${pid}" 2>/dev/null; then
        echo "Stopping ${name} (PID: ${pid})..."
        kill "${pid}" 2>/dev/null || true
        sleep 10
        if kill -0 "${pid}" 2>/dev/null; then
            echo "Force killing ${name} (PID: ${pid})..."
            kill -9 "${pid}" 2>/dev/null || true
        fi
    fi
}

cleanup() {
    local exit_code=$?
    set +e
    stop_process "${JAVA_PID}" "java-tron"
    stop_process "${RUST_PID}" "rust-backend"
    exit "${exit_code}"
}

on_interrupt() {
    echo ""
    echo "Received interrupt; stopping services..."
    exit 130
}

trap cleanup EXIT
trap on_interrupt INT TERM

SHUTDOWN_HEIGHT=$(get_shutdown_height "main_net_config_remote.conf")

echo "Starting remote execution + remote storage result collection..."
if [ "${SLEEP_DURATION}" -eq 0 ]; then
    echo "Sleep duration: 0 seconds (no timeout; waiting for java-tron to exit)"
else
    echo "Sleep duration: ${SLEEP_DURATION} seconds ($(($SLEEP_DURATION / 60)) minutes)"
fi
echo "Embedded Java log: ${EMBEDDED_JAVA_LOG}"
echo "Embedded CSV: ${EMBEDDED_CSV}"
echo "ResourceSync debug: ${REMOTE_RESOURCE_SYNC_DEBUG}"
echo "ResourceSync confirm: ${REMOTE_RESOURCE_SYNC_CONFIRM}"
if [ -n "${SHUTDOWN_HEIGHT}" ]; then
    echo "Configured shutdown BlockHeight: ${SHUTDOWN_HEIGHT}"
fi

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
     -Dremote.resource.sync.debug=${REMOTE_RESOURCE_SYNC_DEBUG} -Dremote.resource.sync.confirm=${REMOTE_RESOURCE_SYNC_CONFIRM} \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "REMOTE" >> start.log 2>&1 &
JAVA_PID=$!

echo "Java-tron started with PID: $JAVA_PID"
echo "Rust-backend started with PID: $RUST_PID"

# Step 6: Wait for java-tron to exit (preferred) or configured timeout, then stop services
echo "Current time: $(date '+%Y-%m-%d %H:%M:%S')"
if [ "${SLEEP_DURATION}" -eq 0 ]; then
    echo "Waiting for java-tron to exit (no timeout; SLEEP_DURATION=0)..."
    print_wait_status
    last_progress=${SECONDS}
    while kill -0 $JAVA_PID 2>/dev/null; do
        if [ $((SECONDS - last_progress)) -ge "${PROGRESS_INTERVAL}" ]; then
            print_wait_status
            last_progress=${SECONDS}
        fi
        sleep "${WAIT_INTERVAL}"
    done
else
    echo "Waiting up to ${SLEEP_DURATION} seconds for java-tron to exit..."
    end_time=$((SECONDS + SLEEP_DURATION))
    print_wait_status
    last_progress=${SECONDS}
    while kill -0 $JAVA_PID 2>/dev/null; do
        if [ "${SECONDS}" -ge "${end_time}" ]; then
            echo "Timeout reached; java-tron still running."
            break
        fi
        if [ $((SECONDS - last_progress)) -ge "${PROGRESS_INTERVAL}" ]; then
            print_wait_status
            last_progress=${SECONDS}
        fi
        sleep "${WAIT_INTERVAL}"
    done
fi

echo "Step 6: Stopping services..."
stop_process "${JAVA_PID}" "java-tron"
stop_process "${RUST_PID}" "rust-backend"

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

if [ -f "${EMBEDDED_JAVA_LOG}" ]; then
    echo "Embedded Java log already present: ${EMBEDDED_JAVA_LOG}"
elif [ -f "../archive/${EMBEDDED_JAVA_LOG}" ]; then
    cp "../archive/${EMBEDDED_JAVA_LOG}" ./
else
    echo "Warning: embedded Java log not found: ${EMBEDDED_JAVA_LOG}"
fi

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
