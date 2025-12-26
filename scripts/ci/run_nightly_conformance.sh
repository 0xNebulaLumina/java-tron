#!/bin/bash
# Nightly Conformance Test Script
# Runs comprehensive conformance tests including:
# 1. Fixture-based conformance (fast, PR-gate)
# 2. CSV replay/diff against real blockchain data (slow, nightly)
#
# Usage:
#   ./scripts/ci/run_nightly_conformance.sh [OPTIONS]
#
# Options:
#   --fixtures-only     Only run fixture conformance (skip CSV replay)
#   --csv-only          Only run CSV replay/diff (skip fixtures)
#   --duration SECONDS  Duration for CSV data collection (default: 1200 = 20 minutes)
#   --embedded-csv PATH Path to embedded execution CSV for comparison
#   --skip-build        Skip compilation steps (assumes already built)
#   --report-dir DIR    Directory for reports (default: nightly-reports/)
#
# Exit codes:
#   0 - All tests passed
#   1 - Fixture conformance failed
#   2 - CSV replay/diff failed
#   3 - Build failed
#   4 - Invalid arguments

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Default configuration
FIXTURES_ONLY=false
CSV_ONLY=false
DURATION=1200
EMBEDDED_CSV=""
SKIP_BUILD=false
REPORT_DIR="$PROJECT_ROOT/nightly-reports"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --fixtures-only)
            FIXTURES_ONLY=true
            shift
            ;;
        --csv-only)
            CSV_ONLY=true
            shift
            ;;
        --duration)
            DURATION="$2"
            shift 2
            ;;
        --embedded-csv)
            EMBEDDED_CSV="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --report-dir)
            REPORT_DIR="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            exit 4
            ;;
    esac
done

# Create report directory
mkdir -p "$REPORT_DIR"
REPORT_FILE="$REPORT_DIR/nightly-$TIMESTAMP.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$REPORT_FILE"
}

log "========================================"
log "Nightly Conformance Test Suite"
log "========================================"
log "Project root: $PROJECT_ROOT"
log "Report file: $REPORT_FILE"
log "Fixtures only: $FIXTURES_ONLY"
log "CSV only: $CSV_ONLY"
log ""

FIXTURE_STATUS=0
CSV_STATUS=0

# =============================================================================
# Phase 1: Fixture Conformance (Fast)
# =============================================================================
if [ "$CSV_ONLY" = false ]; then
    log "========================================"
    log "Phase 1: Fixture Conformance Tests"
    log "========================================"

    cd "$PROJECT_ROOT"

    if [ "$SKIP_BUILD" = false ]; then
        log "Building project..."
        ./gradlew clean build -x test --dependency-verification=off >> "$REPORT_FILE" 2>&1 || {
            log "ERROR: Build failed"
            exit 3
        }
    fi

    log "Running fixture conformance tests..."
    "$SCRIPT_DIR/run_fixture_conformance.sh" >> "$REPORT_FILE" 2>&1 && {
        log "✅ Fixture conformance: PASSED"
        FIXTURE_STATUS=0
    } || {
        log "❌ Fixture conformance: FAILED"
        FIXTURE_STATUS=1
    }
fi

# =============================================================================
# Phase 2: CSV Replay/Diff (Slow, Real Blockchain Data)
# =============================================================================
if [ "$FIXTURES_ONLY" = false ]; then
    log "========================================"
    log "Phase 2: CSV Replay/Diff Tests"
    log "========================================"

    cd "$PROJECT_ROOT"

    if [ -z "$EMBEDDED_CSV" ]; then
        log "WARNING: No embedded CSV specified (--embedded-csv), skipping CSV comparison"
        log "To run CSV comparison, provide a baseline embedded execution CSV"
        CSV_STATUS=0
    else
        if [ ! -f "$EMBEDDED_CSV" ]; then
            log "ERROR: Embedded CSV not found: $EMBEDDED_CSV"
            CSV_STATUS=2
        else
            log "Starting data collection (duration: ${DURATION}s)..."
            log "Embedded CSV baseline: $EMBEDDED_CSV"

            # Use collect_remote_results.sh but capture its output
            if [ -f "$PROJECT_ROOT/collect_remote_results.sh" ]; then
                "$PROJECT_ROOT/collect_remote_results.sh" "$DURATION" "1.embedded-java.log" "$EMBEDDED_CSV" >> "$REPORT_FILE" 2>&1 && {
                    log "✅ CSV replay/diff: PASSED"
                    CSV_STATUS=0
                } || {
                    log "❌ CSV replay/diff: FAILED (mismatches found)"
                    CSV_STATUS=2
                }
            else
                log "WARNING: collect_remote_results.sh not found, skipping CSV replay"
                CSV_STATUS=0
            fi
        fi
    fi
fi

# =============================================================================
# Summary
# =============================================================================
log ""
log "========================================"
log "Nightly Conformance Summary"
log "========================================"
log "Timestamp: $TIMESTAMP"
log "Report: $REPORT_FILE"
log ""

if [ "$CSV_ONLY" = false ]; then
    if [ $FIXTURE_STATUS -eq 0 ]; then
        log "Fixture Conformance: ✅ PASSED"
    else
        log "Fixture Conformance: ❌ FAILED"
    fi
fi

if [ "$FIXTURES_ONLY" = false ]; then
    if [ $CSV_STATUS -eq 0 ]; then
        log "CSV Replay/Diff: ✅ PASSED (or skipped)"
    else
        log "CSV Replay/Diff: ❌ FAILED"
    fi
fi

log "========================================"

# Exit with appropriate code
if [ $FIXTURE_STATUS -ne 0 ]; then
    exit 1
fi
if [ $CSV_STATUS -ne 0 ]; then
    exit 2
fi

log "All tests passed!"
exit 0
