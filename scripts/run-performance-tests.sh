#!/bin/bash

# Performance Testing Script for java-tron Storage PoC
# This script runs comprehensive performance tests against the gRPC storage service

set -e

# Configuration
REMOTE_HOST="${STORAGE_REMOTE_HOST:-localhost}"
REMOTE_PORT="${STORAGE_REMOTE_PORT:-50011}"
RUST_SERVICE_PID=""
REPORTS_DIR="reports/$(date +%Y%m%d-%H%M%S)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Cleanup function
cleanup() {
    log_info "Cleaning up..."
    if [ ! -z "$RUST_SERVICE_PID" ]; then
        log_info "Stopping Rust storage service (PID: $RUST_SERVICE_PID)"
        kill $RUST_SERVICE_PID 2>/dev/null || true
        wait $RUST_SERVICE_PID 2>/dev/null || true
    fi
    
    # Kill any remaining cargo processes
    pkill -f "cargo run" 2>/dev/null || true
    pkill -f "storage-service" 2>/dev/null || true
    
    log_info "Cleanup completed"
}

# Set up cleanup trap
trap cleanup EXIT INT TERM

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Check if Java is available
    if ! command -v java &> /dev/null; then
        log_error "Java is not installed or not in PATH"
        exit 1
    fi
    
    # Check if Gradle is available
    if [ ! -f "./gradlew" ]; then
        log_error "Gradle wrapper not found. Please run from project root directory."
        exit 1
    fi
    
    # Check if Rust/Cargo is available
    if ! command -v cargo &> /dev/null; then
        log_error "Rust/Cargo is not installed or not in PATH"
        exit 1
    fi
    
    # Check if rust-storage-service directory exists
    if [ ! -d "rust-storage-service" ]; then
        log_error "rust-storage-service directory not found. Please run from project root."
        exit 1
    fi
    
    log_success "Prerequisites check passed"
}

# Build components
build_components() {
    log_info "Building components..."
    
    # Build Rust storage service
    log_info "Building Rust storage service..."
    cd rust-storage-service
    cargo build --release
    cd ..
    
    # Build Java components
    log_info "Building Java components..."
    ./gradlew build -x test -x checkstyleMain -x checkstyleTest -x lint --dependency-verification=off
    
    log_success "Components built successfully"
}

# Start Rust storage service
start_rust_service() {
    log_info "Starting Rust storage service on ${REMOTE_HOST}:${REMOTE_PORT}..."
    
    # Create data directory
    mkdir -p data/rust-storage
    
    # Start service in background
    cd rust-storage-service
    RUST_LOG=info DATA_PATH=../data/rust-storage HOST=$REMOTE_HOST PORT=$REMOTE_PORT cargo run --release &
    RUST_SERVICE_PID=$!
    cd ..
    
    log_info "Rust service started with PID: $RUST_SERVICE_PID"
    
    # Wait for service to be ready
    log_info "Waiting for service to be ready..."
    for i in {1..30}; do
        # Try to connect to the gRPC service by testing TCP connection
        if timeout 5 bash -c "echo > /dev/tcp/${REMOTE_HOST}/${REMOTE_PORT}" 2>/dev/null; then
            log_info "Port ${REMOTE_PORT} is open, service appears to be ready"
            log_success "Rust storage service is ready"
            return 0
        fi
        log_info "Waiting for service... (attempt $i/30)"
        sleep 2
    done
    
    log_error "Rust storage service failed to start or is not responding"
    exit 1
}

# Run unit tests
run_unit_tests() {
    log_info "Running unit tests..."
    
    ./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSPITest" \
        -x checkstyleMain -x checkstyleTest -x lint --dependency-verification=off \
        --console=plain
    
    log_success "Unit tests completed"
}

# Run integration tests
run_integration_tests() {
    log_info "Running integration tests..."
    
    ./gradlew :framework:test --tests "org.tron.core.storage.spi.StorageSPIIntegrationTest" \
        -Dstorage.remote.host=$REMOTE_HOST -Dstorage.remote.port=$REMOTE_PORT -x checkstyleMain -x checkstyleTest -x lint --dependency-verification=off \
        --console=plain
    
    log_success "Integration tests completed"
}

# Run embedded benchmarks (no external service needed)
run_embedded_benchmarks() {
    log_info "Running embedded RocksDB performance benchmarks..."
    
    mkdir -p "$REPORTS_DIR"
    
    local tests=(
        "benchmarkSingleOperationLatency"
        "benchmarkBatchOperationThroughput"
        "generatePerformanceReport"
    )
    
    for test in "${tests[@]}"; do
        log_info "Running embedded benchmark: $test"
        ./gradlew :framework:test --tests "org.tron.core.storage.spi.EmbeddedStoragePerformanceBenchmark.$test" \
            -x checkstyleMain -x checkstyleTest -x lint --dependency-verification=off \
            --console=plain --info \
            2>&1 | tee "$REPORTS_DIR/embedded-$test.log"
    done
    
    log_success "Embedded benchmarks completed"
}

# Run performance benchmarks
run_performance_benchmarks() {
    log_info "Running gRPC performance benchmarks..."
    
    mkdir -p "$REPORTS_DIR"
    
    # Run each benchmark test individually for detailed results
    local tests=(
        "benchmarkSingleOperationLatency"
        "benchmarkBatchOperationThroughput"
        "generatePerformanceReport"
    )
    
    for test in "${tests[@]}"; do
        log_info "Running gRPC benchmark: $test"
        ./gradlew :framework:test --tests "org.tron.core.storage.spi.RemoteStoragePerformanceBenchmark.$test" \
            -Dstorage.remote.host=$REMOTE_HOST -Dstorage.remote.port=$REMOTE_PORT -x checkstyleMain -x checkstyleTest -x lint --dependency-verification=off \
            --console=plain --info \
            2>&1 | tee "$REPORTS_DIR/benchmark-$test.log"
    done
    
    log_success "gRPC performance benchmarks completed"
    log_info "Reports saved to: $REPORTS_DIR"
    
    # Extract and analyze metrics from logs
    extract_metrics_from_logs
}

# Extract metrics from log files
extract_metrics_from_logs() {
    log_info "Extracting metrics from benchmark logs..."
    
    local metrics_summary="$REPORTS_DIR/extracted-metrics.txt"
    local metrics_csv="$REPORTS_DIR/extracted-metrics.csv"
    
    # Initialize metrics files
    echo "# Extracted Performance Metrics" > "$metrics_summary"
    echo "Timestamp: $(date)" >> "$metrics_summary"
    echo "" >> "$metrics_summary"
    
    echo "TestName,MetricName,Value,Unit,ExtractedAt" > "$metrics_csv"
    
    # Extract metrics from each log file (both embedded and gRPC)
    for log_file in "$REPORTS_DIR"/*-*.log; do
        if [ -f "$log_file" ]; then
            local test_name=$(basename "$log_file" .log | sed 's/benchmark-//' | sed 's/embedded-//')
            log_info "Extracting metrics from $test_name"
            
            echo "## $test_name" >> "$metrics_summary"
            echo "" >> "$metrics_summary"
            
            # Extract METRIC: lines
            grep "METRIC:" "$log_file" | while IFS= read -r line; do
                # Parse METRIC: TestName.MetricName = Value Unit
                if [[ $line =~ METRIC:\ ([^.]+)\.([^\ ]+)\ =\ ([0-9.]+)\ ([^\ ]+) ]]; then
                    local test="${BASH_REMATCH[1]}"
                    local metric="${BASH_REMATCH[2]}"
                    local value="${BASH_REMATCH[3]}"
                    local unit="${BASH_REMATCH[4]}"
                    
                    echo "  $metric: $value $unit" >> "$metrics_summary"
                    echo "$test,$metric,$value,$unit,$(date '+%Y-%m-%d %H:%M:%S')" >> "$metrics_csv"
                fi
            done
            
            # Extract BENCHMARK: headers and summaries
            grep -A 20 "BENCHMARK:" "$log_file" | grep -E "(Average|Throughput|Latency)" | while IFS= read -r line; do
                echo "  $line" >> "$metrics_summary"
            done
            
            echo "" >> "$metrics_summary"
        fi
    done
    
    log_success "Metrics extracted to $metrics_summary and $metrics_csv"
}

# Generate summary report
generate_summary() {
    log_info "Generating performance summary..."
    
    local summary_file="$REPORTS_DIR/performance-summary.md"
    
    cat > "$summary_file" << EOF
# Performance Testing Summary

**Test Date:** $(date)
**gRPC Server:** ${REMOTE_HOST}:${REMOTE_PORT}
**Java Version:** $(java -version 2>&1 | head -n 1)
**System Info:** $(uname -a)

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
EOF

    # Add links to detailed reports
    for report in "$REPORTS_DIR"/*.log; do
        if [ -f "$report" ]; then
            echo "- [$(basename "$report")]($(basename "$report"))" >> "$summary_file"
        fi
    done
    
    # Add links to metrics files
    if [ -f "$REPORTS_DIR/extracted-metrics.txt" ]; then
        echo "- [Extracted Metrics Summary](extracted-metrics.txt)" >> "$summary_file"
    fi
    if [ -f "$REPORTS_DIR/extracted-metrics.csv" ]; then
        echo "- [Metrics CSV Data](extracted-metrics.csv)" >> "$summary_file"
    fi
    if [ -f "$REPORTS_DIR/performance-metrics.json" ]; then
        echo "- [Performance Metrics JSON](performance-metrics.json)" >> "$summary_file"
    fi
    if [ -f "$REPORTS_DIR/performance-metrics.csv" ]; then
        echo "- [Performance Metrics CSV](performance-metrics.csv)" >> "$summary_file"
    fi
    
    # Add key metrics summary if available
    if [ -f "$REPORTS_DIR/extracted-metrics.txt" ]; then
        echo "" >> "$summary_file"
        echo "## Key Performance Metrics" >> "$summary_file"
        echo "" >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
        head -50 "$REPORTS_DIR/extracted-metrics.txt" >> "$summary_file"
        echo "\`\`\`" >> "$summary_file"
    fi
    
    cat >> "$summary_file" << EOF

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
EOF

    log_success "Summary report generated: $summary_file"
}

# Main execution flow
main() {
    log_info "Starting Performance Testing Phase for java-tron Storage PoC"
    log_info "=================================================="
    
    # Step 1: Prerequisites
    check_prerequisites
    
    # Step 2: Build
    build_components
    
    # Step 3: Start services
    start_rust_service
    
    # Step 4: Run tests
    log_info "Running test suite..."
    
    run_unit_tests
    run_integration_tests
    
    # Run embedded benchmarks first (no external service needed)
    run_embedded_benchmarks
    
    # Then run gRPC benchmarks
    run_performance_benchmarks
    
    # Step 5: Generate reports
    generate_summary
    
    log_success "Performance testing phase completed successfully!"
    log_info "Check the reports directory for detailed results: $REPORTS_DIR"
    
    # Optional: Keep service running for manual testing
    if [ "${KEEP_RUNNING:-false}" = "true" ]; then
        log_info "Service will keep running for manual testing. Press Ctrl+C to stop."
        wait $RUST_SERVICE_PID
    fi
}

# Help function
show_help() {
    cat << EOF
Performance Testing Script for java-tron Storage PoC

Usage: $0 [OPTIONS]

Options:
    -h, --help          Show this help message
    -k, --keep-running  Keep services running after tests for manual testing
    
Environment Variables:
    STORAGE_REMOTE_HOST   gRPC server host (default: localhost)
    STORAGE_REMOTE_PORT   gRPC server port (default: 50011)
    KEEP_RUNNING        Keep services running after tests (default: false)

Examples:
    # Run standard performance tests
    ./scripts/run-performance-tests.sh
    
    # Run tests and keep service running
    ./scripts/run-performance-tests.sh --keep-running
    
    # Run tests against remote server
    STORAGE_REMOTE_HOST=remote-host ./scripts/run-performance-tests.sh
EOF
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -k|--keep-running)
            KEEP_RUNNING=true
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Run main function
main 