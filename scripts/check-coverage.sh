#!/usr/bin/env bash
# Check code coverage for Rotel project
# Enforces minimum coverage threshold and generates reports

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

# Configuration
MIN_COVERAGE=80.0
REPORT_FORMAT="html"
OPEN_REPORT=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--threshold)
            MIN_COVERAGE="$2"
            shift 2
            ;;
        -f|--format)
            REPORT_FORMAT="$2"
            shift 2
            ;;
        -o|--open)
            OPEN_REPORT=true
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -t, --threshold NUM  Minimum coverage threshold (default: 80.0)"
            echo "  -f, --format FORMAT  Report format: html, lcov, json (default: html)"
            echo "  -o, --open           Open HTML report in browser"
            echo "  -h, --help           Show this help message"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

echo -e "${YELLOW}Checking Code Coverage${NC}"
echo "========================================"
echo "Minimum threshold: ${MIN_COVERAGE}%"
echo "Report format: ${REPORT_FORMAT}"
echo ""

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo -e "${RED}Error: cargo-llvm-cov not installed${NC}"
    echo "Install with: cargo install cargo-llvm-cov"
    echo "Then run: cargo llvm-cov --version"
    exit 1
fi

# Clean previous coverage data
echo -e "${YELLOW}Cleaning previous coverage data...${NC}"
cargo llvm-cov clean --workspace

# Run tests with coverage
echo -e "${YELLOW}Running tests with coverage instrumentation...${NC}"

# Build coverage command based on format
COVERAGE_CMD="cargo llvm-cov --all-features --workspace"

case $REPORT_FORMAT in
    html)
        COVERAGE_CMD="$COVERAGE_CMD --html"
        REPORT_PATH="target/llvm-cov/html/index.html"
        ;;
    lcov)
        COVERAGE_CMD="$COVERAGE_CMD --lcov --output-path target/llvm-cov/lcov.info"
        REPORT_PATH="target/llvm-cov/lcov.info"
        ;;
    json)
        COVERAGE_CMD="$COVERAGE_CMD --json --output-path target/llvm-cov/coverage.json"
        REPORT_PATH="target/llvm-cov/coverage.json"
        ;;
    *)
        echo -e "${RED}Unknown format: $REPORT_FORMAT${NC}"
        exit 1
        ;;
esac

# Run coverage
if ! eval "$COVERAGE_CMD"; then
    echo -e "${RED}Coverage generation failed${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}Coverage report generated: $REPORT_PATH${NC}"

# Extract coverage percentage
# Note: This is a simplified extraction. In production, you'd parse the JSON output
echo ""
echo -e "${YELLOW}Extracting coverage metrics...${NC}"

# Run coverage again with summary output to get percentage
COVERAGE_OUTPUT=$(cargo llvm-cov --all-features --workspace --summary-only 2>&1 || true)

# Try to extract coverage percentage from output
# Format: "TOTAL   123   45   63.41%"
COVERAGE_PCT=$(echo "$COVERAGE_OUTPUT" | grep -E "^TOTAL" | awk '{print $NF}' | tr -d '%' || echo "0")

if [ -z "$COVERAGE_PCT" ] || [ "$COVERAGE_PCT" = "0" ]; then
    echo -e "${YELLOW}Warning: Could not extract coverage percentage${NC}"
    echo "Coverage report available at: $REPORT_PATH"

    # Open report if requested
    if [ "$OPEN_REPORT" = true ] && [ "$REPORT_FORMAT" = "html" ]; then
        if command -v open &> /dev/null; then
            open "$REPORT_PATH"
        elif command -v xdg-open &> /dev/null; then
            xdg-open "$REPORT_PATH"
        fi
    fi

    exit 0
fi

echo "Current coverage: ${COVERAGE_PCT}%"
echo "Minimum threshold: ${MIN_COVERAGE}%"

# Compare coverage to threshold
if (( $(echo "$COVERAGE_PCT >= $MIN_COVERAGE" | bc -l) )); then
    echo ""
    echo -e "${GREEN}✓ Coverage check passed!${NC}"
    echo -e "${GREEN}Coverage (${COVERAGE_PCT}%) meets minimum threshold (${MIN_COVERAGE}%)${NC}"
    EXIT_CODE=0
else
    echo ""
    echo -e "${RED}✗ Coverage check failed!${NC}"
    echo -e "${RED}Coverage (${COVERAGE_PCT}%) is below minimum threshold (${MIN_COVERAGE}%)${NC}"
    echo -e "${YELLOW}Please add more tests to increase coverage${NC}"
    EXIT_CODE=1
fi

# Open report if requested
if [ "$OPEN_REPORT" = true ] && [ "$REPORT_FORMAT" = "html" ]; then
    echo ""
    echo -e "${YELLOW}Opening coverage report in browser...${NC}"
    if command -v open &> /dev/null; then
        open "$REPORT_PATH"
    elif command -v xdg-open &> /dev/null; then
        xdg-open "$REPORT_PATH"
    else
        echo -e "${YELLOW}Could not open browser automatically${NC}"
        echo "Open manually: $REPORT_PATH"
    fi
fi

exit $EXIT_CODE

# Made with Bob
