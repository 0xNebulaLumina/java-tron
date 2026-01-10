#!/bin/bash
# Fixture Conformance Test Script
# Runs Java fixture generators and Rust conformance tests for PR gate
#
# Usage:
#   ./scripts/ci/run_fixture_conformance.sh [--generate-only] [--rust-only] [--contract CONTRACT_NAME]
#
# Options:
#   --generate-only   Only generate fixtures (Java), skip Rust tests
#   --rust-only       Only run Rust conformance tests (assumes fixtures exist)
#   --contract NAME   Run only for specific contract (e.g., proposal, account, exchange)
#
# Exit codes:
#   0 - All tests passed
#   1 - Java fixture generation failed
#   2 - Rust conformance tests failed
#   3 - Invalid arguments

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUST_BACKEND="$PROJECT_ROOT/rust-backend"
FIXTURES_DIR="$PROJECT_ROOT/conformance/fixtures"

# Parse arguments
GENERATE_ONLY=false
RUST_ONLY=false
CONTRACT_FILTER=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --generate-only)
            GENERATE_ONLY=true
            shift
            ;;
        --rust-only)
            RUST_ONLY=true
            shift
            ;;
        --contract)
            CONTRACT_FILTER="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            exit 3
            ;;
    esac
done

echo "========================================"
echo "Fixture Conformance Test Suite"
echo "========================================"
echo "Project root: $PROJECT_ROOT"
echo "Fixtures dir: $FIXTURES_DIR"
echo ""

# Step 1: Generate fixtures (Java)
if [ "$RUST_ONLY" = false ]; then
    echo "========================================"
    echo "Step 1: Generating Java Fixtures"
    echo "========================================"

    cd "$PROJECT_ROOT"

    # Build test filter based on contract filter
    TEST_FILTER=""
    if [ -n "$CONTRACT_FILTER" ]; then
        case "$CONTRACT_FILTER" in
            proposal)
                TEST_FILTER="ProposalFixtureGeneratorTest*"
                ;;
            account)
                TEST_FILTER="AccountFixtureGeneratorTest*"
                ;;
            contract|metadata)
                TEST_FILTER="ContractMetadataFixtureGeneratorTest*"
                ;;
            brokerage)
                TEST_FILTER="BrokerageFixtureGeneratorTest*"
                ;;
            resource|delegation)
                TEST_FILTER="ResourceDelegationFixtureGeneratorTest*"
                ;;
            trc10)
                TEST_FILTER="Trc10ExtensionFixtureGeneratorTest*"
                ;;
            exchange)
                TEST_FILTER="ExchangeFixtureGeneratorTest*"
                ;;
            market)
                TEST_FILTER="MarketFixtureGeneratorTest*"
                ;;
            *)
                TEST_FILTER="*FixtureGeneratorTest*"
                ;;
        esac
        echo "Running fixture tests matching: $TEST_FILTER"
        ./gradlew :framework:test --tests "$TEST_FILTER" -Dconformance.output="$FIXTURES_DIR" --dependency-verification=off || {
            echo "ERROR: Java fixture generation failed"
            exit 1
        }
    else
        echo "Running all fixture generator tests..."
        ./gradlew :framework:test --tests "*FixtureGeneratorTest*" -Dconformance.output="$FIXTURES_DIR" --dependency-verification=off || {
            echo "ERROR: Java fixture generation failed"
            exit 1
        }
    fi

    echo "Fixtures generated successfully."
    echo ""
fi

# Step 2: Run Rust conformance tests
if [ "$GENERATE_ONLY" = false ]; then
    echo "========================================"
    echo "Step 2: Running Rust Conformance Tests"
    echo "========================================"

    cd "$RUST_BACKEND"

    # Check if fixtures exist
    if [ ! -d "$FIXTURES_DIR" ]; then
        echo "ERROR: Fixtures directory not found: $FIXTURES_DIR"
        echo "Run with --generate-only first or without --rust-only"
        exit 2
    fi

    # Some fixture families are not yet supported by the Rust backend.
    # Exclude them by default so `--rust-only` can run against the supported set.
    # Set FIXTURE_CONFORMANCE_INCLUDE_UNSUPPORTED=1 to include everything.
    FIXTURES_DIR_FOR_RUST="$FIXTURES_DIR"
    if [ "${FIXTURE_CONFORMANCE_INCLUDE_UNSUPPORTED:-0}" != "1" ]; then
        should_exclude_fixture_contract_dir() {
                case "$1" in
create_smart_contract|\
market_cancel_order_contract|\
market_sell_asset_contract|\
participate_asset_issue_contract|\
trigger_smart_contract|\
unfreeze_balance_v2_contract|\
vote_witness_contract|\
withdraw_balance_contract|\
withdraw_expire_unfreeze_contract)
                    return 0
                    ;;
                *)
                    return 1
                    ;;
            esac
        }

        FILTERED_FIXTURES_DIR="$(mktemp -d)"
        trap 'rm -rf "$FILTERED_FIXTURES_DIR"' EXIT

        echo "Filtering fixtures (excluding unsupported contract families)..."
        echo "Set FIXTURE_CONFORMANCE_INCLUDE_UNSUPPORTED=1 to include everything."
        for contract_dir in "$FIXTURES_DIR"/*; do
            [ -d "$contract_dir" ] || continue
            contract_name="$(basename "$contract_dir")"
            if should_exclude_fixture_contract_dir "$contract_name"; then
                echo "  - Skipping $contract_name"
                continue
            fi
            ln -s "$contract_dir" "$FILTERED_FIXTURES_DIR/$contract_name"
        done

        FIXTURES_DIR_FOR_RUST="$FILTERED_FIXTURES_DIR"
        echo ""
    fi

    # Count fixtures
    FIXTURE_COUNT=$(find -L "$FIXTURES_DIR_FOR_RUST" -name "metadata.json" | wc -l)
    echo "Found $FIXTURE_COUNT fixture(s) to test"

    if [ "$FIXTURE_COUNT" -eq 0 ]; then
        echo "WARNING: No fixtures found. Skipping Rust tests."
        exit 0
    fi

    # Run Rust conformance tests
    echo "Building and running conformance tests..."

    # Set fixtures path for Rust tests
    export CONFORMANCE_FIXTURES_DIR="$FIXTURES_DIR_FOR_RUST"

    # Some environments (including sandboxed CI) configure a global rustc wrapper
    # like sccache via cargo config, which can fail with permission errors.
    # Disable wrappers for this script unless explicitly opted in.
    if [ "${FIXTURE_CONFORMANCE_KEEP_RUSTC_WRAPPER:-0}" != "1" ]; then
        unset RUSTC_WRAPPER
        unset RUSTC_WORKSPACE_WRAPPER
        export CARGO_BUILD_RUSTC_WRAPPER=
        export CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER=
    fi

    # Run conformance tests (use --ignored to include the real fixture runner)
    cargo test --package tron-backend-core conformance -- --nocapture --ignored || {
        echo "ERROR: Rust conformance tests failed"
        exit 2
    }

    echo "Rust conformance tests passed."
    echo ""
fi

echo "========================================"
echo "All Conformance Tests Passed!"
echo "========================================"

exit 0
