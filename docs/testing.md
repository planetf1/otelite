# Testing Guide

This guide covers testing practices, tools, and workflows for Rotel development.

## Table of Contents

- [Overview](#overview)
- [Test Types](#test-types)
- [Running Tests](#running-tests)
- [Writing Tests](#writing-tests)
- [Code Coverage](#code-coverage)
- [Performance Testing](#performance-testing)
- [CI/CD Integration](#cicd-integration)
- [Troubleshooting](#troubleshooting)

## Overview

Rotel uses a comprehensive testing strategy to ensure code quality and reliability:

- **Unit Tests**: Test individual functions and modules
- **Integration Tests**: Test component interactions
- **E2E Tests**: Test complete workflows
- **Doc Tests**: Test code examples in documentation

### Testing Requirements

- **Minimum Coverage**: 80% code coverage
- **Strict Mode**: All tests must pass (no retries)
- **Performance**: Unit tests must complete in <30 seconds
- **Isolation**: Tests must be independent and repeatable

## Test Types

### Unit Tests

**Purpose**: Test individual functions and modules in isolation

**Location**: `src/` files with `#[cfg(test)]` modules

**Example**:
```rust
// crates/rotel-core/src/lib.rs
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 2), 4);
        assert_eq!(add(-1, 1), 0);
        assert_eq!(add(0, 0), 0);
    }
}
```

**Best Practices**:
- Test edge cases and boundary conditions
- Use descriptive test names
- Keep tests focused and simple
- Mock external dependencies

### Integration Tests

**Purpose**: Test interactions between components

**Location**: `tests/integration/` directory

**Example**:
```rust
// tests/integration/receiver_test.rs
use rotel_receiver::OtlpReceiver;
use rotel_storage::EmbeddedStorage;

#[tokio::test]
async fn test_receiver_storage_integration() {
    let storage = EmbeddedStorage::new("test_data").await.unwrap();
    let receiver = OtlpReceiver::new(storage);

    // Send test data
    let metrics = create_test_metrics();
    receiver.ingest_metrics(metrics).await.unwrap();

    // Verify storage
    let stored = storage.query_metrics(query).await.unwrap();
    assert_eq!(stored.len(), 1);
}
```

**Best Practices**:
- Test realistic component interactions
- Use test fixtures for consistent data
- Clean up resources after tests
- Test error handling and edge cases

### E2E Tests

**Purpose**: Test complete user workflows

**Location**: `tests/e2e/` directory

**Example**:
```rust
// tests/e2e/full_pipeline_test.rs
#[tokio::test]
async fn test_full_telemetry_pipeline() {
    // Start Rotel server
    let server = start_test_server().await;

    // Send OTLP data
    let client = create_otlp_client();
    client.send_metrics(test_metrics()).await.unwrap();

    // Query via API
    let response = query_api("/api/metrics").await;
    assert_eq!(response.status(), 200);

    // Verify data in response
    let metrics: Vec<Metric> = response.json().await.unwrap();
    assert_eq!(metrics.len(), 1);

    // Cleanup
    server.shutdown().await;
}
```

**Best Practices**:
- Test complete user scenarios
- Use realistic test data
- Verify end-to-end behavior
- Clean up test resources

### Doc Tests

**Purpose**: Test code examples in documentation

**Location**: Inline in rustdoc comments

**Example**:
```rust
/// Adds two numbers together.
///
/// # Examples
///
/// ```
/// use rotel_core::add;
///
/// let result = add(2, 2);
/// assert_eq!(result, 4);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

**Best Practices**:
- Include examples in all public APIs
- Test examples as part of test suite
- Keep examples simple and focused
- Show common use cases

## Running Tests

### Basic Test Execution

```bash
# Run all tests
cargo test

# Run tests in specific crate
cargo test -p rotel-core

# Run specific test
cargo test test_add

# Run tests with output
cargo test -- --nocapture

# Run tests in parallel (default)
cargo test -- --test-threads=4
```

### Using cargo-nextest (Faster)

```bash
# Install cargo-nextest
cargo install cargo-nextest

# Run all tests (60% faster)
cargo nextest run

# Run with output
cargo nextest run --no-capture

# Run specific test
cargo nextest run test_add
```

### Test Scripts

```bash
# Run all tests with coverage
./scripts/run-tests.sh

# Check coverage threshold
./scripts/check-coverage.sh

# Run specific test suite
./scripts/run-tests.sh integration
```

### Filtering Tests

```bash
# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Run only doc tests
cargo test --doc

# Run tests matching pattern
cargo test receiver
```

## Writing Tests

### Test Structure

Follow the Arrange-Act-Assert pattern:

```rust
#[test]
fn test_feature() {
    // Arrange: Set up test data and dependencies
    let input = create_test_input();
    let expected = create_expected_output();

    // Act: Execute the code under test
    let result = function_under_test(input);

    // Assert: Verify the result
    assert_eq!(result, expected);
}
```

### Assertions

```rust
// Equality
assert_eq!(actual, expected);
assert_ne!(actual, unexpected);

// Boolean
assert!(condition);
assert!(!condition);

// Panic
#[should_panic]
#[test]
fn test_panic() {
    panic!("Expected panic");
}

// Result
#[test]
fn test_result() -> Result<(), Error> {
    let result = fallible_function()?;
    assert_eq!(result, expected);
    Ok(())
}
```

### Test Fixtures

Create reusable test data:

```rust
// tests/common/mod.rs
pub fn create_test_metrics() -> Vec<Metric> {
    vec![
        Metric {
            name: "test_metric".to_string(),
            value: 42.0,
            timestamp: 1234567890,
            attributes: HashMap::new(),
        }
    ]
}

// tests/integration/test.rs
use crate::common::create_test_metrics;

#[test]
fn test_with_fixtures() {
    let metrics = create_test_metrics();
    // Use metrics in test
}
```

### Async Tests

```rust
#[tokio::test]
async fn test_async_function() {
    let result = async_function().await;
    assert_eq!(result, expected);
}

#[tokio::test]
async fn test_with_timeout() {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        async_function()
    ).await;

    assert!(result.is_ok());
}
```

### Mocking

```rust
use mockall::predicate::*;
use mockall::*;

#[automock]
trait Storage {
    fn save(&self, data: &str) -> Result<()>;
}

#[test]
fn test_with_mock() {
    let mut mock = MockStorage::new();
    mock.expect_save()
        .with(eq("test"))
        .times(1)
        .returning(|_| Ok(()));

    let result = function_using_storage(&mock);
    assert!(result.is_ok());
}
```

## Code Coverage

### Generating Coverage Reports

```bash
# Install cargo-llvm-cov
cargo install cargo-llvm-cov

# Generate HTML report
cargo llvm-cov --all-features --workspace --html

# Open report
open target/llvm-cov/html/index.html

# Generate text summary
cargo llvm-cov --all-features --workspace

# Generate JSON report
cargo llvm-cov --all-features --workspace --json --output-path coverage.json
```

### Coverage Requirements

- **Minimum**: 80% overall coverage
- **Critical paths**: 100% coverage for security and data integrity code
- **New code**: Must maintain or improve coverage percentage

### Checking Coverage

```bash
# Check if coverage meets threshold
./scripts/check-coverage.sh

# Output:
# Current coverage: 85.2%
# Threshold: 80.0%
# ✅ Coverage check passed
```

### Coverage in CI

Coverage is automatically checked in CI:

```yaml
- name: Check coverage
  run: |
    cargo llvm-cov --all-features --workspace --json --output-path coverage.json
    ./scripts/check-coverage.sh
```

## Performance Testing

### Benchmarking

```rust
// benches/benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_function(c: &mut Criterion) {
    c.bench_function("my_function", |b| {
        b.iter(|| my_function(black_box(42)))
    });
}

criterion_group!(benches, benchmark_function);
criterion_main!(benches);
```

Run benchmarks:

```bash
cargo bench
```

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Unit tests | <30s | Total execution time |
| Integration tests | <60s | Total execution time |
| E2E tests | <120s | Total execution time |
| Pre-commit | <10s | All hooks |
| CI pipeline | <10min | Full pipeline |

### Profiling

```bash
# Install flamegraph
cargo install flamegraph

# Generate flamegraph
cargo flamegraph --test integration_tests

# Open flamegraph.svg in browser
```

## CI/CD Integration

### GitHub Actions Workflow

Tests run automatically on:
- Push to any branch
- Pull requests
- Daily schedule (2 AM UTC)

### CI Test Jobs

1. **Unit Tests**: Fast feedback on code changes
2. **Integration Tests**: Verify component interactions
3. **E2E Tests**: Validate complete workflows
4. **Coverage**: Ensure coverage threshold met
5. **Linting**: Check code quality
6. **Security**: Scan for vulnerabilities

### Local CI Simulation

```bash
# Run all CI checks locally
./scripts/run-ci-checks.sh

# This runs:
# - cargo fmt --check
# - cargo clippy --all-targets --all-features -- -D warnings
# - cargo test --all-features --workspace
# - cargo llvm-cov --all-features --workspace
# - ./scripts/check-coverage.sh
```

## Troubleshooting

### Tests Failing Locally

1. **Clean build**:
   ```bash
   cargo clean
   cargo test
   ```

2. **Update dependencies**:
   ```bash
   cargo update
   cargo test
   ```

3. **Check Rust version**:
   ```bash
   rustc --version
   # Should be 1.77+
   ```

### Tests Passing Locally but Failing in CI

1. **Check environment differences**:
   - OS differences (macOS vs Linux)
   - Rust version differences
   - Dependency version differences

2. **Run tests in Docker** (simulates CI):
   ```bash
   docker run --rm -v $(pwd):/workspace -w /workspace rust:1.77 cargo test
   ```

3. **Check for race conditions**:
   ```bash
   # Run tests multiple times
   for i in {1..10}; do cargo test || break; done
   ```

### Slow Tests

1. **Identify slow tests**:
   ```bash
   cargo nextest run --no-capture | grep "SLOW"
   ```

2. **Profile tests**:
   ```bash
   cargo test -- --nocapture --test-threads=1
   ```

3. **Optimize or parallelize**:
   - Use `cargo nextest` for parallel execution
   - Mock expensive operations
   - Use test fixtures instead of setup code

### Coverage Issues

1. **Missing coverage**:
   - Add tests for uncovered code
   - Remove dead code
   - Mark intentionally untested code with `#[cfg(not(tarpaulin_include))]`

2. **Coverage too low**:
   ```bash
   # Generate detailed report
   cargo llvm-cov --all-features --workspace --html
   open target/llvm-cov/html/index.html
   # Identify uncovered lines and add tests
   ```

## Best Practices

1. **Write tests first** (TDD): Define expected behavior before implementation
2. **Keep tests simple**: One assertion per test when possible
3. **Use descriptive names**: Test names should describe what they test
4. **Test edge cases**: Boundary conditions, empty inputs, error cases
5. **Avoid test interdependence**: Tests should run in any order
6. **Clean up resources**: Use `Drop` or explicit cleanup
7. **Mock external dependencies**: Keep tests fast and reliable
8. **Document test intent**: Add comments explaining complex test logic

## Resources

- [Rust Testing Documentation](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [cargo-nextest](https://nexte.st/)
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)
- [mockall](https://docs.rs/mockall/)
- [criterion](https://docs.rs/criterion/)

---

**Next**: [Troubleshooting Guide](troubleshooting.md) | [Contributing Guide](../CONTRIBUTING.md)
