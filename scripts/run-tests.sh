#!/usr/bin/env bash
# Run all tests for Rotel project
# This script runs unit tests, integration tests, and doc tests

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

echo -e "${YELLOW}Running Rotel Test Suite${NC}"
echo "========================================"

# Parse arguments
VERBOSE=false
COVERAGE=false
NEXTEST=false

while [[ $# -gt 0 ]]; do
    case $1 in
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -c|--coverage)
            COVERAGE=true
            shift
            ;;
        -n|--nextest)
            NEXTEST=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -v, --verbose    Show verbose test output"
            echo "  -c, --coverage   Run with coverage reporting"
            echo "  -n, --nextest    Use cargo-nextest for faster execution"
            echo "  -h, --help       Show this help message"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

# Function to run tests
run_tests() {
    local test_type=$1
    local test_cmd=$2

    echo ""
    echo -e "${YELLOW}Running $test_type...${NC}"

    if eval "$test_cmd"; then
        echo -e "${GREEN}✓ $test_type passed${NC}"
        return 0
    else
        echo -e "${RED}✗ $test_type failed${NC}"
        return 1
    fi
}

# Track failures
FAILED=0

# Build test command based on options
if [ "$COVERAGE" = true ]; then
    echo -e "${YELLOW}Running tests with coverage...${NC}"
    if ! command -v cargo-llvm-cov &> /dev/null; then
        echo -e "${RED}Error: cargo-llvm-cov not installed${NC}"
        echo "Install with: cargo install cargo-llvm-cov"
        exit 1
    fi

    cargo llvm-cov --all-features --workspace --html
    echo -e "${GREEN}Coverage report generated in target/llvm-cov/html/index.html${NC}"
    exit 0
fi

# Choose test runner
if [ "$NEXTEST" = true ]; then
    if ! command -v cargo-nextest &> /dev/null; then
        echo -e "${YELLOW}Warning: cargo-nextest not installed, falling back to cargo test${NC}"
        TEST_CMD="cargo test"
    else
        TEST_CMD="cargo nextest run"
    fi
else
    TEST_CMD="cargo test"
fi

# Add verbose flag if requested
if [ "$VERBOSE" = true ]; then
    TEST_CMD="$TEST_CMD -- --nocapture"
fi

# Run unit tests
if ! run_tests "Unit Tests" "$TEST_CMD --lib"; then
    FAILED=$((FAILED + 1))
fi

# Run integration tests
if ! run_tests "Integration Tests" "$TEST_CMD --test '*'"; then
    FAILED=$((FAILED + 1))
fi

# Run doc tests
if ! run_tests "Doc Tests" "cargo test --doc"; then
    FAILED=$((FAILED + 1))
fi

# Summary
echo ""
echo "========================================"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}$FAILED test suite(s) failed${NC}"
    exit 1
fi

# Made with Bob
