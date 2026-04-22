#!/usr/bin/env bash
#
# Development Environment Setup Script for Otelite
#
# This script automates the setup of the development environment including:
# - Rust toolchain verification
# - Development tools installation (cargo-nextest, cargo-llvm-cov, gitleaks)
# - Pre-commit hooks configuration
# - Workspace verification
#
# Usage: ./scripts/setup-dev.sh

set -e  # Exit on error
set -u  # Exit on undefined variable

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

success() {
    echo -e "${GREEN}✓${NC} $1"
}

warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

error() {
    echo -e "${RED}✗${NC} $1"
}

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Main setup function
main() {
    echo ""
    info "Setting up Otelite development environment..."
    echo ""

    # Step 1: Verify Rust installation
    info "Step 1/6: Verifying Rust installation..."
    if ! command_exists rustc; then
        error "Rust is not installed"
        echo ""
        echo "Please install Rust from https://rustup.rs/"
        echo "Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    RUST_VERSION=$(rustc --version | awk '{print $2}')
    info "Found Rust version: $RUST_VERSION"

    # Check minimum Rust version (1.77)
    REQUIRED_VERSION="1.77.0"
    if ! printf '%s\n' "$REQUIRED_VERSION" "$RUST_VERSION" | sort -V -C; then
        error "Rust version $RUST_VERSION is too old (minimum: $REQUIRED_VERSION)"
        echo ""
        echo "Please update Rust:"
        echo "  rustup update stable"
        exit 1
    fi
    success "Rust version is compatible"
    echo ""

    # Step 2: Install Rust components
    info "Step 2/6: Installing Rust components..."

    if rustup component list | grep -q "rustfmt.*installed"; then
        success "rustfmt already installed"
    else
        info "Installing rustfmt..."
        rustup component add rustfmt
        success "rustfmt installed"
    fi

    if rustup component list | grep -q "clippy.*installed"; then
        success "clippy already installed"
    else
        info "Installing clippy..."
        rustup component add clippy
        success "clippy installed"
    fi

    if rustup component list | grep -q "llvm-tools.*installed"; then
        success "llvm-tools-preview already installed"
    else
        info "Installing llvm-tools-preview..."
        rustup component add llvm-tools-preview
        success "llvm-tools-preview installed"
    fi
    echo ""

    # Step 3: Install development tools
    info "Step 3/6: Installing development tools..."

    # Install cargo-nextest
    if command_exists cargo-nextest; then
        success "cargo-nextest already installed"
    else
        info "Installing cargo-nextest..."
        cargo install cargo-nextest --locked
        success "cargo-nextest installed"
    fi

    # Install cargo-llvm-cov
    if command_exists cargo-llvm-cov; then
        success "cargo-llvm-cov already installed"
    else
        info "Installing cargo-llvm-cov..."
        cargo install cargo-llvm-cov --locked
        success "cargo-llvm-cov installed"
    fi

    # Install gitleaks
    if command_exists gitleaks; then
        success "gitleaks already installed"
    else
        info "Installing gitleaks..."
        if [[ "$OSTYPE" == "darwin"* ]]; then
            # macOS
            if command_exists brew; then
                brew install gitleaks
            else
                warning "Homebrew not found. Please install gitleaks manually:"
                echo "  https://github.com/gitleaks/gitleaks#installing"
            fi
        elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
            # Linux
            warning "Please install gitleaks manually:"
            echo "  https://github.com/gitleaks/gitleaks#installing"
        else
            warning "Unsupported OS. Please install gitleaks manually:"
            echo "  https://github.com/gitleaks/gitleaks#installing"
        fi

        if command_exists gitleaks; then
            success "gitleaks installed"
        else
            warning "gitleaks not installed. Secret detection will not work."
        fi
    fi
    echo ""

    # Step 4: Install pre-commit
    info "Step 4/6: Installing pre-commit..."
    if command_exists pre-commit; then
        success "pre-commit already installed"
    else
        info "Installing pre-commit..."
        if command_exists pip3; then
            pip3 install pre-commit
        elif command_exists pip; then
            pip install pre-commit
        elif command_exists brew; then
            brew install pre-commit
        else
            error "Could not install pre-commit. Please install manually:"
            echo "  pip install pre-commit"
            echo "  or"
            echo "  brew install pre-commit"
            exit 1
        fi
        success "pre-commit installed"
    fi
    echo ""

    # Step 5: Configure pre-commit hooks
    info "Step 5/7: Configuring pre-commit hooks..."
    if [ -f ".git/hooks/pre-commit" ]; then
        success "pre-commit hooks already configured"
    else
        info "Installing pre-commit hooks..."
        pre-commit install
        success "pre-commit hooks installed"
    fi
    echo ""

    # Step 6: Set up beads issue tracking (optional)
    info "Step 6/7: Setting up beads issue tracking..."
    if ! command_exists bd; then
        warning "beads (bd) not found — issue tracking integration will be inactive"
        echo ""
        echo "  To install beads, see: https://github.com/steveyegge/beads"
        echo "  After installing, re-run this script or run: bd doctor && bd dolt pull"
        echo ""
    else
        info "Running bd doctor to install hook integration..."
        bd doctor 2>/dev/null || true
        info "Pulling issue data from remote..."
        bd dolt pull 2>/dev/null || warning "Could not pull beads data (remote may not be configured yet)"
        success "beads set up"
    fi
    echo ""

    # Step 7: Verify workspace
    info "Step 7/7: Verifying workspace..."

    info "Checking workspace compilation..."
    if cargo check --workspace --all-features; then
        success "Workspace compiles successfully"
    else
        error "Workspace compilation failed"
        exit 1
    fi

    info "Running tests..."
    if cargo test --workspace; then
        success "All tests pass"
    else
        error "Some tests failed"
        exit 1
    fi

    info "Running linters..."
    if cargo clippy --workspace --all-targets --all-features -- -D warnings; then
        success "No clippy warnings"
    else
        error "Clippy found issues"
        exit 1
    fi

    if cargo fmt -- --check; then
        success "Code is properly formatted"
    else
        warning "Code needs formatting. Run: cargo fmt"
    fi
    echo ""

    # Summary
    echo ""
    success "Development environment setup complete!"
    echo ""
    echo "Next steps:"
    echo "  1. Run tests:              cargo test"
    echo "  2. Run tests (faster):     cargo nextest run"
    echo "  3. Check coverage:         ./scripts/check-coverage.sh"
    echo "  4. Format code:            cargo fmt"
    echo "  5. Run linter:             cargo clippy --all-targets --all-features -- -D warnings"
    echo "  6. Run pre-commit:         pre-commit run --all-files"
    echo "  7. View open issues:       bd ready     (requires beads)"
    echo ""
    echo "For more information, see:"
    echo "  - README.md"
    echo "  - CONTRIBUTING.md"
    echo "  - docs/quickstart.md"
    echo ""
}

# Run main function
main "$@"
