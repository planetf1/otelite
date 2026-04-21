# Rotel

**Lightweight OpenTelemetry receiver and dashboard for local development**

Rotel is a single-binary observability tool that receives OpenTelemetry data (logs, traces, metrics) and provides a web dashboard and CLI for viewing it. Designed for local LLM development with minimal resource usage (<100MB memory, <5% CPU), it starts in seconds and requires no external dependencies.

## Quick Start

```bash
# Development build and run
cargo run --bin rotel-cli -- dashboard

# Production build (optimised, single binary at target/release/rotel-cli)
cargo build --release --bin rotel-cli

# Install to PATH
cargo install --path crates/rotel-cli
rotel-cli dashboard
```

> **Note:** The binary will be renamed to `rotel` in an upcoming release. For now it is `rotel-cli`.

The dashboard starts three services:
- **OTLP gRPC receiver** on `localhost:4317`
- **OTLP HTTP receiver** on `localhost:4318`
- **Web UI and REST API** on `http://localhost:3000`

## Features

- **🚀 Fast**: Starts in <3s, <100MB memory, <5% CPU idle
- **📊 Full OTLP Support**: Metrics, logs, and traces via gRPC (4317) and HTTP (4318)
- **💾 Embedded Storage**: SQLite-based, no external database required
- **🌐 Web Dashboard**: View and filter telemetry data at `http://localhost:3000`
- **⌨️ Terminal UI**: Full-featured TUI with `rotel tui`
- **🔧 CLI**: Query and export data with `rotel logs`, `rotel traces`, `rotel metrics`
- **📦 Single Binary**: Zero runtime dependencies

## Documentation

- [**Quick Start Guide**](docs/quickstart.md) - Installation and first run
- [**Architecture**](ARCHITECTURE.md) - System design and components
- [**TUI Guide**](docs/tui-quickstart.md) - Terminal user interface
- [**Testing Guide**](docs/testing.md) - Running and writing tests
- [**Contributing**](CONTRIBUTING.md) - Development workflow

## Sending Data

### Using otel-cli (easiest for testing)

```bash
# Install otel-cli
go install github.com/equinix-labs/otel-cli@latest

# Send a test trace
otel-cli exec --endpoint http://localhost:4318 --protocol http/protobuf -- echo "hello"
```

### Python

```python
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

trace.set_tracer_provider(TracerProvider())
otlp_exporter = OTLPSpanExporter(endpoint="http://localhost:4317", insecure=True)
trace.get_tracer_provider().add_span_processor(BatchSpanProcessor(otlp_exporter))

tracer = trace.get_tracer(__name__)
with tracer.start_as_current_span("my-operation"):
    # Your code here
    pass
```

### Rust

```rust
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

### JavaScript/Node.js

```javascript
const { NodeSDK } = require('@opentelemetry/sdk-node');
const { OTLPTraceExporter } = require('@opentelemetry/exporter-trace-otlp-grpc');

const sdk = new NodeSDK({
  traceExporter: new OTLPTraceExporter({ url: 'http://localhost:4317' }),
});
sdk.start();
```

### Go

```go
import (
    "go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
    "go.opentelemetry.io/otel/sdk/trace"
)

exporter, _ := otlptracegrpc.New(ctx,
    otlptracegrpc.WithEndpoint("localhost:4317"),
    otlptracegrpc.WithInsecure(),
)
tp := trace.NewTracerProvider(trace.WithBatcher(exporter))
```

## CLI Usage

```bash
# List recent logs
rotel-cli logs list --severity ERROR --since 1h

# Search logs
rotel-cli logs search "database timeout"

# Show a specific log
rotel-cli logs show <timestamp>

# List traces with duration filter
rotel-cli traces list --min-duration 1s

# Show trace details
rotel-cli traces show <trace-id>

# List metrics
rotel-cli metrics list --name "http_*"

# Show specific metric
rotel-cli metrics show http_requests_total

# JSON output for scripting
rotel-cli --format json logs list | jq '.[] | select(.severity == "ERROR")'
```

## Terminal UI

```bash
# Start TUI
rotel-cli tui

# Connect to custom API
rotel-cli tui --api-url http://localhost:3000
```

**Keyboard shortcuts:**
- `l` - Logs view
- `t` - Traces view
- `m` - Metrics view
- `/` - Search
- `f` - Filter
- `?` - Help
- `q` - Quit

See [TUI documentation](docs/tui-quickstart.md) for details.

## REST API

```bash
# List logs
curl "http://localhost:3000/api/logs?severity=ERROR&limit=50"

# Get specific log
curl "http://localhost:3000/api/logs/<timestamp>"

# List traces
curl "http://localhost:3000/api/traces?min_duration_ns=1000000"

# Get trace with spans
curl "http://localhost:3000/api/traces/<trace-id>"

# List metrics
curl "http://localhost:3000/api/metrics?name=http_requests_total"

# Health check
curl "http://localhost:3000/api/health"
```

## Development

```bash
# Clone and build
git clone https://github.com/YOUR_USERNAME/rotel.git
cd rotel
cargo build --workspace

# Run tests
cargo test --workspace

# Run quality gates
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check

# Run with coverage
cargo llvm-cov --all-features --workspace --html

# Enforce the workspace coverage threshold locally
./scripts/check-coverage.sh --threshold 80
```

Coverage reports are generated for every pull request in GitHub Actions. The CI workflow uploads LCOV results to Codecov for badge/trend reporting, publishes HTML/JSON coverage artifacts for inspection, comments the workspace percentage on pull requests, and enforces per-crate minimums: `rotel-cli` 75%, `rotel-core` 85%, `rotel-receiver` 80%, `rotel-storage` 85%, `rotel-server` 70%, and `rotel-tui` 70%.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow.

## Architecture

```
┌─────────────────────────────────────────────┐
│         Web Dashboard (port 3000)           │
│         + REST API (rotel-server)           │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│       SQLite Storage (rotel-storage)        │
│            with FTS5 search                 │
└─────────────────▲───────────────────────────┘
                  │
┌─────────────────┴───────────────────────────┐
│       OTLP Receivers (rotel-receiver)       │
│    gRPC (4317) + HTTP (4318)                │
└─────────────────────────────────────────────┘
```

**Crate structure:**
- `rotel-core` - Domain types (LogRecord, Span, Metric)
- `rotel-storage` - SQLite backend with async trait
- `rotel-receiver` - OTLP gRPC and HTTP ingest
- `rotel-server` - REST API and web UI
- `rotel-cli` - Command-line interface (integrates all components)
- `rotel-tui` - Terminal user interface

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed design.

## Performance

| Metric | Target | Typical |
|--------|--------|---------|
| Memory (idle) | <100MB | ~50MB |
| CPU (idle) | <5% | ~2% |
| Startup time | <3s | ~1.5s |
| Throughput | 1000 events/s | 2000+ events/s |

## Project Status

**Current version:** 0.1.0-alpha

Rotel is in active development. The API may change between releases.

See [CHANGELOG.md](CHANGELOG.md) for release history.

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for:
- Development workflow
- Testing requirements
- Code style guidelines
- Pull request process

## License

Apache License 2.0 - see [LICENSE](LICENSE)

## Support

- **Issues**: [GitHub Issues](https://github.com/YOUR_USERNAME/rotel/issues)
- **Discussions**: [GitHub Discussions](https://github.com/YOUR_USERNAME/rotel/discussions)
- **Security**: See [SECURITY.md](SECURITY.md)

---

**Made with ❤️ for the LLM community**
