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
# Dashboard port via --addr; OTLP ports via environment variables
OTELITE_OTLP_GRPC_PORT=14317 OTELITE_OTLP_HTTP_PORT=14318 \
  otelite start --addr 127.0.0.1:18080
```

The dashboard IP also controls where the OTLP receivers bind, so
`otelite start --addr 0.0.0.0:3000` publishes OTLP on all interfaces
while the default (`127.0.0.1:3000`) keeps them local-only.

### Permission Denied

**Problem**: "Permission denied" when accessing data directory

**Solution**:
```bash
# Create data directory with correct permissions
mkdir -p ~/.otelite/data
chmod 755 ~/.otelite/data

# Or specify a different data directory
otelite start --storage-path /tmp/otelite-data
# (OTELITE_DATA_DIR does the same for an already-running setup)
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
Otelite has no tunable memory limits — memory scales with the data in
`~/.otelite/data` and the active purge window. To reduce it:

```bash
# Shorten the retention window (1-365, default 90), then restart:
OTELITE_RETENTION_DAYS=7 otelite start

# Purge everything immediately (all signals), then restart:
curl -X POST http://127.0.0.1:3000/api/admin/purge
otelite restart
```

### Otelite Crashes on Startup

**Problem**: Otelite exits immediately after starting

**Diagnosis**:
```bash
# Run with verbose logging
otelite start --log-level debug

# Check logs (daily-rotated: otelite.log.YYYY-MM-DD)
tail -f ~/.otelite/data/otelite.log.*
```

**Common Causes**:
1. **Invalid flags or environment**: Otelite takes no config file at
   runtime (the generated `~/.config/otelite/config.toml` is not read
   back) — check the `--addr`/`--storage-path` values and the
   `OTELITE_*` environment variables against `otelite start --help`
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
Otelite has no rate-limiting knobs. Usual culprits, in order:

1. **Retention purges** — large databases purge in bursts; shorten
   `OTELITE_RETENTION_DAYS` so purges are smaller.
2. **Dashboard polling** — an open dashboard refreshes continuously;
   close tabs you are not using.
3. **Log volume** — run the daemon with a quieter log level
   (`otelite start` inherits `RUST_LOG`; set `RUST_LOG=warn`).

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
# Retention is automatic: data older than OTELITE_RETENTION_DAYS
# (default 90, range 1-365) is purged by the daemon's scheduler.
# Set it for the daemon environment (e.g. the launchd/systemd unit
# or your shell), then restart:
OTELITE_RETENTION_DAYS=7 otelite restart

# Or purge everything immediately (all signals):
curl -X POST http://127.0.0.1:3000/api/admin/purge
```

### Corrupted Database

**Problem**: "Database corruption detected"

**Solution**:
```bash
# Backup data
cp -r ~/.otelite/data ~/.otelite/data.backup

# Diagnose with sqlite3 (if installed)
sqlite3 ~/.otelite/data/otelite.db "PRAGMA integrity_check;"

# If the database is unrecoverable, delete it and restart
# (telemetry older than this point is lost)
rm -rf ~/.otelite/data
otelite start
```

### Slow Writes

**Problem**: Data ingestion is slow

**Solution**:
Writes are already batched: each export (a batch of logs, spans or
metrics) is stored in a single transaction, and writes wait for a
running retention purge instead of failing. There is no write-tuning
knob to adjust. If ingestion is slow, the usual culprits are:

1. **Small, chatty exports** — fewer, larger batches write less.
2. **Slow disk** — `iotop -p $(pgrep otelite)` while ingesting.
3. **A purge in progress** — writes pause briefly during a purge;
   a smaller `OTELITE_RETENTION_DAYS` makes purges shorter.

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

# Runtime configuration (flags/environment, there is no config file)
env | grep -E '^(OTELITE_|RUST_LOG)'
ps aux | grep -v grep | grep "otelite serve"   # effective flags

# Logs (last 100 lines; daily-rotated)
tail -n 100 ~/.otelite/data/otelite.log.*

# Resource usage
ps aux | grep otelite
df -h ~/.otelite/data
```

### Enabling Debug Logging

```bash
# Foreground: RUST_LOG is picked up by the server directly
RUST_LOG=debug otelite serve

# Specific modules
RUST_LOG=otelite_receiver=debug,otelite_storage=trace otelite serve

# Daemon: the daemon inherits the environment of the process that
# starts it (note: `otelite start --log-level` only affects the
# short-lived start command, not the daemon)
export RUST_LOG=otelite=debug
otelite restart
```

The daemon's tracing goes to the daily-rotated log
(`~/.otelite/data/otelite.log.YYYY-MM-DD`), so after `otelite
restart`, tail that file rather than the terminal.

### Reporting Bugs

1. **Search existing issues**: Check [GitHub Issues](https://github.com/planetf1/otelite/issues)
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

**Solution**: Reduce the ingestion rate — send fewer, larger OTLP
batches from the exporter, or spread exports over more exporters.
There is no queue-size knob to tune.

### "Invalid metric name"

**Cause**: Metric name doesn't follow OpenTelemetry conventions

**Solution**: Use valid metric names (alphanumeric, dots, underscores)

### "Trace ID not found"

**Cause**: Querying for non-existent trace

**Solution**: Verify trace ID and time range

## Performance Tuning

Otelite exposes no pipeline/storage tuning knobs — writes are batched
per export in a single transaction and the storage engine is SQLite in
WAL mode. The levers that actually move the needle:

### Throughput

- Send fewer, larger OTLP export batches from the exporter (each
  batch is one transaction on the otelite side).
- Keep `~/.otelite/data` on fast storage (SSD); `iotop -p $(pgrep otelite)`
  while ingesting tells you if the disk is the bottleneck.

### Query latency

- Narrow the time range and use filters (session, model, error level)
  so queries scan less.
- Shorten `OTELITE_RETENTION_DAYS` (1-365, default 90) so scans cover
  less history.

### Memory and disk

- Retention is the main lever: shorter `OTELITE_RETENTION_DAYS` means
  less stored data, smaller memory and disk footprint, and shorter
  purges.
- `curl -X POST http://127.0.0.1:3000/api/admin/purge` purges all
  signals immediately.

---

**Still having issues?** Open a [GitHub Issue](https://github.com/planetf1/otelite/issues).
