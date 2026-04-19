#!/bin/bash
set -e

# Use Homebrew LLVM tools which support newer profile format
HOMEBREW_LLVM="/opt/homebrew/opt/llvm/bin"

if [ ! -f "$HOMEBREW_LLVM/llvm-profdata" ]; then
    echo "Error: Homebrew LLVM not found. Install with: brew install llvm"
    exit 1
fi

export LLVM_PROFDATA="$HOMEBREW_LLVM/llvm-profdata"
export LLVM_COV="$HOMEBREW_LLVM/llvm-cov"

echo "Using Homebrew LLVM tools from: $HOMEBREW_LLVM"
echo "LLVM_PROFDATA: $LLVM_PROFDATA"
echo "LLVM_COV: $LLVM_COV"

# Verify versions
echo ""
echo "LLVM version:"
$LLVM_COV --version | head -1

# Clean previous coverage data
echo ""
echo "Cleaning previous coverage data..."
cargo llvm-cov clean

# Generate coverage report
echo ""
echo "Generating coverage report..."
cargo llvm-cov --workspace --html

echo ""
echo "✓ Coverage report generated at: target/llvm-cov/html/index.html"
echo "  Open with: open target/llvm-cov/html/index.html"
