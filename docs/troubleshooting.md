# Troubleshooting Guide

Common issues and solutions for Otelite development and deployment.

## Table of Contents

- [Installation Issues](#installation-issues)
- [Runtime Issues](#runtime-issues)
- [Development Issues](#development-issues)
- [Testing Issues](#testing-issues)
- [Performance Issues](#performance-issues)
- [Network Issues](#network-issues)
- [Storage Issues](#storage-issues)
- [Getting Help](#getting-help)

## Installation Issues

### Rust Version Too Old

**Problem**: Build fails with "requires rustc 1.77 or newer"

**Solution**:
```bash
# Update Rust
rustup update stable

# Verify version
rustc --version
# Should show 1.77 or higher
```

### Cargo Build Fails

**Problem**: `cargo build` fails with dependency errors

**Solution**:
```bash
# Clean build artifacts
cargo clean

# Update dependencies
cargo update

# Rebuild
cargo build --release
```

### Missing System Dependencies

**Problem**: Build fails with "could not find system library"

**Solution (macOS)**:
```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install Homebrew dependencies
brew install pkg-config openssl
```

**Solution (Linux)**:
```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install build-essential pkg-config libssl-dev

# Fedora/RHEL
sudo dnf install gcc pkg-config openssl-devel

# Arch
sudo pacman -S base-devel pkg-config openssl
```

## Runtime Issues

### Port Already in Use

**Problem**: "Address already in use" error when starting Otelite

**Diagnosis**:
```bash
# Check what's using the port
lsof -i :4317
lsof -i :4318
lsof -i :8080
```

**Solution 1**: Stop the conflicting process
```bash
# Kill process using port
kill -9 <PID>
```

**Solution 2**: Use different ports
```bash
# Start with custom ports
otelite start --grpc-port 14317 --http-port 14318 --dashboard-port 18080
```

**Solution 3**: Update configuration
```toml
# otelite.toml
[server]
grpc_port = 14317
http_port = 14318
dashboard_port = 18080
```

### Permission Denied

**Problem**: "Permission denied" when accessing data directory

**Solution**:
```bash
# Create data directory with correct permissions
mkdir -p ~/.otelite/data
chmod 755 ~/.otelite/data

# Or specify different directory
otelite start --data-dir /tmp/otelite-data
```

### High Memory Usage

**Problem**: Otelite using more than 100MB memory

**Diagnosis**:
```bash
# Check memory usage
ps aux | grep otelite

# Or use top
top -p $(pgrep otelite)
```

**Solution**:
```toml
# otelite.toml - Reduce memory limits
[limits]
max_memory_mb = 50
max_queue_size = 5000
batch_size = 500
```

### Otelite Crashes on Startup

**Problem**: Otelite exits immediately after starting

**Diagnosis**:
```bash
# Run with verbose logging
otelite start --log-level debug

# Check logs
tail -f ~/.otelite/logs/otelite.log
```

**Common Causes**:
1. **Invalid configuration**: Check `otelite.toml` syntax
2. **Corrupted data**: Delete `~/.otelite/data` and restart
3. **Missing permissions**: Check file/directory permissions

## Development Issues

### Pre-commit Hooks Failing

**Problem**: Pre-commit hooks fail on commit

**Diagnosis**:
```bash
# Run hooks manually
pre-commit run --all-files
```

**Solution 1**: Fix formatting
```bash
cargo fmt
git add .
git commit
```

**Solution 2**: Fix linting errors
```bash
cargo clippy --fix --all-targets --all-features
git add .
git commit
```

**Solution 3**: Skip hooks (not recommended)
```bash
git commit --no-verify
```

### Clippy Warnings

**Problem**: `cargo clippy` reports warnings

**Solution**:
```bash
# Auto-fix where possible
cargo clippy --fix --all-targets --all-features

# Or suppress specific warnings (use sparingly)
#[allow(clippy::warning_name)]
fn my_function() {
    // code
}
```

### Rustfmt Errors

**Problem**: `cargo fmt` fails or produces unexpected formatting

**Solution**:
```bash
# Check rustfmt version
rustfmt --version

# Update rustfmt
rustup component add rustfmt

# Format with stable features only
cargo fmt
```

### IDE Not Recognizing Code

**Problem**: VS Code/IntelliJ shows errors but code compiles

**Solution (VS Code)**:
```bash
# Restart Rust Analyzer
# Command Palette (Cmd+Shift+P): "Rust Analyzer: Restart Server"

# Or reload window
# Command Palette: "Developer: Reload Window"
```

**Solution (IntelliJ)**:
```bash
# Invalidate caches
# File > Invalidate Caches / Restart
```

## Testing Issues

### Tests Failing Intermittently

**Problem**: Tests pass sometimes, fail other times

**Common Causes**:
1. **Race conditions**: Tests depend on timing
2. **Shared state**: Tests modify global state
3. **Resource leaks**: Tests don't clean up properly

**Solution**:
```bash
# Run tests sequentially
cargo test -- --test-threads=1

# Run specific test multiple times
for i in {1..10}; do cargo test test_name || break; done
```

### Coverage Below Threshold

**Problem**: Coverage check fails with "Coverage below 80%"

**Diagnosis**:
```bash
# Generate detailed coverage report
cargo llvm-cov --all-features --workspace --html
open target/llvm-cov/html/index.html
```

**Solution**:
1. Identify uncovered lines in HTML report
2. Add tests for uncovered code
3. Remove dead code
4. Mark intentionally untested code:
   ```rust
   #[cfg(not(tarpaulin_include))]
   fn internal_helper() {
       // Not included in coverage
   }
   ```

### Tests Timeout

**Problem**: Tests hang or timeout

**Solution**:
```bash
# Run with timeout
cargo test -- --test-threads=1 --nocapture

# Or use tokio timeout
#[tokio::test]
async fn test_with_timeout() {
    tokio::time::timeout(
        Duration::from_secs(5),
        async_function()
    ).await.unwrap();
}
```

## Performance Issues

### Slow Test Execution

**Problem**: Tests take longer than 30 seconds

**Solution**:
```bash
# Use cargo-nextest (60% faster)
cargo nextest run

# Run tests in parallel
cargo test -- --test-threads=8

# Profile tests
cargo test -- --nocapture --test-threads=1
```

### High CPU Usage

**Problem**: Otelite using excessive CPU

**Diagnosis**:
```bash
# Check CPU usage
top -p $(pgrep otelite)

# Profile with flamegraph
cargo flamegraph --bin otelite
```

**Solution**:
```toml
# otelite.toml - Reduce processing load
[limits]
max_events_per_second = 500
batch_timeout_ms = 2000
```

### Slow Queries

**Problem**: Dashboard queries take too long

**Solution**:
1. **Add indexes**: Ensure proper indexing on query fields
2. **Reduce time range**: Query smaller time windows
3. **Use aggregations**: Pre-aggregate data for common queries
4. **Enable caching**: Cache frequently accessed data

## Network Issues

### Connection Refused

**Problem**: Application can't connect to Otelite

**Diagnosis**:
```bash
# Check if Otelite is running
ps aux | grep otelite

# Check if ports are listening
netstat -an | grep LISTEN | grep -E '4317|4318|8080'

# Test connection
curl http://localhost:4318/v1/metrics
```

**Solution**:
1. **Start Otelite**: `otelite start`
2. **Check firewall**: Allow ports 4317, 4318, 8080
3. **Verify endpoint**: Use correct URL in application
4. **Check logs**: `otelite logs` for errors

### TLS/SSL Errors

**Problem**: "SSL certificate verify failed"

**Solution**:
```bash
# For development, disable TLS verification (not for production)
export OTEL_EXPORTER_OTLP_INSECURE=true

# Or configure TLS properly
otelite start --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

### Timeout Errors

**Problem**: "Connection timeout" when sending data

**Solution**:
```toml
# Increase timeout in application
[exporter]
timeout_seconds = 30

# Or in Otelite
[server]
request_timeout_seconds = 30
```

## Storage Issues

### Disk Space Full

**Problem**: "No space left on device"

**Diagnosis**:
```bash
# Check disk usage
df -h ~/.otelite/data

# Check data directory size
du -sh ~/.otelite/data
```

**Solution**:
```bash
# Clean old data
otelite clean --older-than 7d

# Or configure retention
# otelite.toml
[storage]
retention_days = 7
max_size_gb = 5
```

### Corrupted Database

**Problem**: "Database corruption detected"

**Solution**:
```bash
# Backup data
cp -r ~/.otelite/data ~/.otelite/data.backup

# Try repair
otelite repair

# If repair fails, delete and restart
rm -rf ~/.otelite/data
otelite start
```

### Slow Writes

**Problem**: Data ingestion is slow

**Solution**:
```toml
# otelite.toml - Optimize write performance
[storage]
batch_size = 2000
batch_timeout_ms = 500
compression = "none"  # Disable compression for speed

[pipeline]
max_queue_size = 20000
```

## Getting Help

### Collecting Debug Information

When reporting issues, include:

```bash
# System information
uname -a
rustc --version
cargo --version

# Otelite version
otelite --version

# Configuration
cat otelite.toml

# Logs (last 100 lines)
tail -n 100 ~/.otelite/logs/otelite.log

# Resource usage
ps aux | grep otelite
df -h ~/.otelite/data
```

### Enabling Debug Logging

```bash
# Start with debug logging
otelite start --log-level debug

# Or set environment variable
export RUST_LOG=debug
otelite start

# For specific modules
export RUST_LOG=otelite_receiver=debug,otelite_storage=trace
otelite start
```

### Reporting Bugs

1. **Search existing issues**: Check [GitHub Issues](https://github.com/YOUR_USERNAME/otelite/issues)
2. **Create new issue**: Use bug report template
3. **Include**:
   - Clear description of problem
   - Steps to reproduce
   - Expected vs actual behavior
   - Debug information (see above)
   - Logs and error messages

### Getting Support

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Questions and general discussion
- **Documentation**: Check [docs](.) for guides
- **Security Issues**: Email security@otelite.dev (do not use public issues)

## Common Error Messages

### "OTLP protocol version mismatch"

**Cause**: Client using incompatible OTLP version

**Solution**: Update client library to OTLP v1.0+

### "Maximum queue size exceeded"

**Cause**: Data ingestion rate exceeds processing capacity

**Solution**: Increase queue size or reduce ingestion rate
```toml
[pipeline]
max_queue_size = 20000
```

### "Invalid metric name"

**Cause**: Metric name doesn't follow OpenTelemetry conventions

**Solution**: Use valid metric names (alphanumeric, dots, underscores)

### "Trace ID not found"

**Cause**: Querying for non-existent trace

**Solution**: Verify trace ID and time range

## Performance Tuning

### Optimize for Throughput

```toml
[pipeline]
batch_size = 2000
batch_timeout_ms = 500
max_queue_size = 20000

[storage]
compression = "none"
write_buffer_size_mb = 64
```

### Optimize for Latency

```toml
[pipeline]
batch_size = 100
batch_timeout_ms = 100
max_queue_size = 5000

[storage]
compression = "zstd"
write_buffer_size_mb = 16
```

### Optimize for Memory

```toml
[limits]
max_memory_mb = 50
max_queue_size = 5000

[pipeline]
batch_size = 500
```

---

**Still having issues?** Open a [GitHub Issue](https://github.com/YOUR_USERNAME/otelite/issues) or start a [Discussion](https://github.com/YOUR_USERNAME/otelite/discussions).
