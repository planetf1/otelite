# Rotel

**OpenTelemetry Receiver & Dashboard for Local LLM Users**

[![CI](https://github.com/YOUR_USERNAME/rotel/workflows/CI/badge.svg)](https://github.com/YOUR_USERNAME/rotel/actions)
[![Security](https://github.com/YOUR_USERNAME/rotel/workflows/Security/badge.svg)](https://github.com/YOUR_USERNAME/rotel/actions)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Rotel is a lightweight, open-source OpenTelemetry receiver and dashboard designed specifically for local LLM users. It provides comprehensive observability for your LLM applications with minimal resource footprint.

## Features

- **🚀 Lightweight**: <100MB memory, <5% CPU idle, starts in <3s
- **📊 Full OTLP Support**: Metrics, logs, and traces via gRPC and HTTP
- **🔌 Pluggable Architecture**: Extensible storage backends and exporters
- **🛡️ Standards Compliant**: Full OpenTelemetry Protocol (OTLP) compliance
- **💾 Embedded Storage**: No external database required (PostgreSQL optional)
- **🌍 Cross-Platform**: macOS (Intel/Apple Silicon) and Linux (x86_64/ARM64)
- **📦 Single Binary**: Zero-dependency deployment

## Quick Start

### Installation

**macOS (Homebrew)**:
```bash
brew install rotel
```

**Linux (Binary)**:
```bash
curl -L https://github.com/YOUR_USERNAME/rotel/releases/latest/download/rotel-linux-x86_64.tar.gz | tar xz
sudo mv rotel /usr/local/bin/
```

**From Source**:
```bash
git clone https://github.com/YOUR_USERNAME/rotel.git
cd rotel
cargo build --release
sudo cp target/release/rotel /usr/local/bin/
```

### Usage

**Start Rotel**:
```bash
rotel start
```

This starts the OTLP receiver on:
- gRPC: `localhost:4317`
- HTTP: `localhost:4318`
- Dashboard: `http://localhost:8080`

**Configure Your Application**:

```rust
// Rust example
use opentelemetry_otlp::WithExportConfig;

let tracer = opentelemetry_otlp::new_pipeline()
    .tracing()
    .with_exporter(
        opentelemetry_otlp::new_exporter()
            .tonic()
            .with_endpoint("http://localhost:4317")
    )
    .install_batch(opentelemetry_sdk::runtime::Tokio)?;
```

```python
# Python example
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

trace.set_tracer_provider(TracerProvider())
otlp_exporter = OTLPSpanExporter(endpoint="http://localhost:4317")
trace.get_tracer_provider().add_span_processor(
    BatchSpanProcessor(otlp_exporter)
)
```

**View Dashboard**:

Open `http://localhost:8080` in your browser to view metrics, logs, and traces.

### Terminal User Interface (TUI)

Rotel includes a powerful terminal-based interface for viewing telemetry data directly in your terminal.

**Installation**:
```bash
cargo install rotel-tui
```

**Usage**:
```bash
# Start TUI (connects to localhost:8080 by default)
rotel-tui

# Connect to custom API endpoint
rotel-tui --api-url http://localhost:8080

# Use custom configuration
rotel-tui --config ~/.config/rotel/tui.toml
```

**Key Features**:
- 📊 **Logs View** (`l` key) - Real-time log streaming with severity filtering and search
- 🔍 **Traces View** (`t` key) - Span waterfall visualization with critical path highlighting
- 📈 **Metrics View** (`m` key) - Time series charts with zoom and navigation
- ⌨️ **Keyboard-Driven** - Complete keyboard navigation, no mouse required
- 🎨 **Color-Coded** - Severity levels, error states, and performance indicators
- 🚀 **Lightweight** - <20MB memory, <1% CPU idle

**Quick Navigation**:
- `l` - Switch to logs view
- `t` - Switch to traces view
- `m` - Switch to metrics view
- `/` - Search within current view
- `f` - Apply filters
- `?` or `h` - Show help screen
- `q` - Quit

See [`docs/tui-quickstart.md`](docs/tui-quickstart.md) for detailed TUI documentation.

### Command-Line Interface (CLI)

Rotel includes a powerful CLI for querying telemetry data from your terminal or scripts.

**Installation**:
```bash
cargo install rotel-cli
```

**Quick Examples**:
```bash
# Query logs
rotel-cli logs list --severity ERROR --since 1h
rotel-cli logs search "database timeout"
rotel-cli logs show log-123

# Query traces
rotel-cli traces list --min-duration 1s --status ERROR
rotel-cli traces show trace-456

# Query metrics
rotel-cli metrics list --name "http_*"
rotel-cli metrics get response_time_ms --label "endpoint=/api/users"

# JSON output for scripting
rotel-cli --format json logs list | jq '.[] | select(.severity == "ERROR")'

# Use in shell scripts
ERROR_COUNT=$(rotel-cli --format json logs list --severity ERROR | jq 'length')
if [ "$ERROR_COUNT" -gt 0 ]; then
  echo "Found $ERROR_COUNT errors"
fi
```

**Key Features**:
- 🚀 **Fast**: <100ms cold start, <1s queries
- 📊 **Multiple Formats**: Pretty tables or JSON output
- 🔧 **Scriptable**: Exit codes, pipeable output, jq-friendly JSON
- 🎨 **Flexible**: Color/header control, custom endpoints, timeouts
- 🌍 **Unix-Friendly**: Follows Unix conventions (stdout/stderr, exit codes)

**Global Flags**:
```bash
--endpoint <URL>        # Backend URL (or ROTEL_ENDPOINT env var)
--format <pretty|json>  # Output format
--no-color              # Disable colors
--no-header             # Disable table headers
--timeout <SECONDS>     # Request timeout
```

See [`specs/004-cli/quickstart.md`](specs/004-cli/quickstart.md) for detailed CLI documentation and scripting examples.

## Configuration

Rotel uses sensible defaults but can be customized via `rotel.toml`:

```toml
[server]
grpc_port = 4317
http_port = 4318
dashboard_port = 8080

[storage]
backend = "embedded"  # or "postgresql"
data_dir = "~/.rotel/data"
max_size_gb = 10

[limits]
max_events_per_second = 1000
max_memory_mb = 100
```

See [`docs/configuration.md`](docs/configuration.md) for full configuration options.

## Documentation

- [**Quick Start Guide**](docs/quickstart.md) - Get up and running in 5 minutes
- [**TUI Quick Start**](docs/tui-quickstart.md) - Terminal UI guide
- [**TUI Keyboard Shortcuts**](docs/tui-shortcuts.md) - Complete shortcuts reference
- [**TUI Troubleshooting**](docs/tui-troubleshooting.md) - TUI-specific issues and solutions
- [**Architecture Overview**](ARCHITECTURE.md) - System design and components
- [**Testing Guide**](docs/testing.md) - Running and writing tests
- [**Contributing Guide**](CONTRIBUTING.md) - How to contribute
- [**Troubleshooting**](docs/troubleshooting.md) - Common issues and solutions
- [**API Documentation**](https://docs.rs/rotel) - Rust API docs

## Development

### Prerequisites

- Rust 1.77+ (stable)
- Git
- Pre-commit (optional but recommended)

### Setup

```bash
# Clone repository
git clone https://github.com/YOUR_USERNAME/rotel.git
cd rotel

# Run setup script (installs tools, configures pre-commit)
./scripts/setup-dev.sh

# Run tests
cargo test

# Run with coverage
cargo llvm-cov --all-features --workspace --html
```

### Testing

```bash
# Run all tests
./scripts/run-tests.sh

# Run specific test suite
cargo test --test integration_tests

# Check coverage
./scripts/check-coverage.sh
```

### Code Quality

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Run pre-commit hooks
pre-commit run --all-files
```

See [`docs/testing.md`](docs/testing.md) for detailed testing documentation.

## Architecture

Rotel is built with a modular, pluggable architecture:

```
┌─────────────────────────────────────────────┐
│              Dashboard UI                    │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│           Query Engine                       │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│         Storage Backend                      │
│  (Embedded / PostgreSQL / Pluggable)        │
└─────────────────▲───────────────────────────┘
                  │
┌─────────────────┴───────────────────────────┐
│          OTLP Receiver                       │
│      (gRPC + HTTP Endpoints)                │
└─────────────────────────────────────────────┘
```

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for detailed architecture documentation.

## Performance

Rotel is designed for minimal resource usage:

| Metric | Target | Typical |
|--------|--------|---------|
| Memory (idle) | <100MB | ~50MB |
| CPU (idle) | <5% | ~2% |
| Startup time | <3s | ~1.5s |
| Throughput | 1000 events/s | 2000+ events/s |
| Test suite | <30s | ~15s |
| Pre-commit | <10s | ~5s |
| CI pipeline | <10min | ~6min |

## Contributing

We welcome contributions! Please see [`CONTRIBUTING.md`](CONTRIBUTING.md) for:

- Code of conduct
- Development workflow
- Pull request process
- Testing requirements
- Code style guidelines

## License

Rotel is licensed under the [Apache License 2.0](LICENSE).

## Project Status

Rotel is currently in **active development**. The API is not yet stable and may change between releases.

Current version: **0.1.0-alpha**

See [`CHANGELOG.md`](CHANGELOG.md) for release history.

## Support

- **Issues**: [GitHub Issues](https://github.com/YOUR_USERNAME/rotel/issues)
- **Discussions**: [GitHub Discussions](https://github.com/YOUR_USERNAME/rotel/discussions)
- **Security**: See [SECURITY.md](SECURITY.md) for reporting vulnerabilities

## Acknowledgments

Rotel is built on top of excellent open-source projects:

- [OpenTelemetry](https://opentelemetry.io/) - Observability framework
- [Tokio](https://tokio.rs/) - Async runtime
- [Tonic](https://github.com/hyperium/tonic) - gRPC implementation
- [Axum](https://github.com/tokio-rs/axum) - Web framework

## Roadmap

See [GitHub Projects](https://github.com/YOUR_USERNAME/rotel/projects) for planned features and milestones.

**Upcoming Features**:
- [ ] Query language for advanced filtering
- [ ] Alerting and notification system
- [ ] Distributed tracing visualization
- [ ] Metrics aggregation and downsampling
- [ ] Plugin marketplace

---

**Made with ❤️ for the LLM community**
