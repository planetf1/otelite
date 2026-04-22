# Otelite

**Lightweight OpenTelemetry receiver and dashboard for local development**

Otelite is a single-binary observability tool that receives OpenTelemetry data (logs, traces, metrics) and provides a web dashboard and terminal UI for viewing it. Designed for local LLM development with minimal resource usage (<100MB memory, <5% CPU), it starts in seconds and requires no external dependencies.

> **Personal project** — developed and maintained on a best-efforts basis by [@planetf1](https://github.com/planetf1). Not an official or supported product. Contributions and feedback welcome via GitHub issues and pull requests.

## Quick Start

### Build and run (development)

```bash
cargo run --bin otelite -- serve
```

### Production build

```bash
# Optimised release binary — ~5x faster startup, much smaller
cargo build --release --bin otelite

# The binary is at:
./target/release/otelite serve

# Install to PATH (then just run `otelite` anywhere)
cargo install --path crates/otelite-cli
otelite serve
```

`otelite serve` starts three services:
- **OTLP gRPC receiver** on `localhost:4317`
- **OTLP HTTP receiver** on `localhost:4318`
- **Web dashboard and REST API** on `http://localhost:3000`

Open `http://localhost:3000` in your browser to view telemetry.

## Features

- **Fast**: Starts in <3s, <100MB memory, <5% CPU idle
- **Full OTLP Support**: Metrics, logs, and traces via gRPC (4317) and HTTP (4318)
- **Embedded Storage**: SQLite-based, no external database required
- **Web Dashboard**: View and filter telemetry data at `http://localhost:3000`
- **Terminal UI**: Full-featured TUI with `otelite tui`
- **CLI**: Query and export data with `otelite logs`, `otelite traces`, `otelite metrics`, `otelite usage`
- **Single Binary**: Zero runtime dependencies
- **GenAI/LLM support**: First-class OTel GenAI semconv — token counts, cache hits, tool calls, model routing

## Screenshots

**Logs** — search, filter by severity, and inspect structured attributes

![Logs list](docs/screenshots/logs-list.png)
![Log detail](docs/screenshots/logs-detail.png)

**Traces** — waterfall view with span-level timing and GenAI attributes (token counts, cache hits, TTFT)

![Traces waterfall](docs/screenshots/traces-list.png)
![Trace span detail](docs/screenshots/traces-detail.png)

**Metrics** — time-series counters and histogram bucket distribution

![Metrics counter](docs/screenshots/metrics-counter.png)
![Metrics histogram](docs/screenshots/metrics-histogram.png)

**Usage** — GenAI/LLM token and request summary by model and provider

![GenAI usage](docs/screenshots/usage.png)

**Setup** — live endpoint display and copy-paste configuration snippets for every SDK

![Setup page](docs/screenshots/setup.png)

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
# Start the server (foreground)
otelite serve

# Start as background daemon
otelite start

# Stop daemon
otelite stop

# Restart daemon (picks up recompiled binary)
otelite restart

# Check daemon status
otelite status

# List recent logs
otelite logs list --severity ERROR --since 1h

# Search logs
otelite logs search "database timeout"

# List traces with duration filter
otelite traces list --min-duration 1s

# Show trace details
otelite traces show <trace-id>

# List metrics
otelite metrics list --name "http_*"

# Token usage summary (GenAI/LLM)
otelite usage --since 24h
otelite usage --since 7d --by-model

# JSON output for scripting
otelite --format json logs list | jq '.[] | select(.severity == "ERROR")'
```

## Terminal UI

```bash
# Start TUI (connects to localhost:3000 by default)
otelite tui

# Connect to custom API URL
otelite tui --api-url http://localhost:3000
```

**Keyboard shortcuts:**
- `l` / `t` / `m` — switch to Logs / Traces / Metrics view
- `Tab` / `Shift+Tab` — cycle between views
- `/` — search
- `f` — filter
- `PageUp` / `PageDown` — scroll
- `?` — help
- `q` — quit

## REST API

```bash
# List logs
curl "http://localhost:3000/api/logs?severity=ERROR&limit=50"

# List traces
curl "http://localhost:3000/api/traces?min_duration_ns=1000000"

# Get trace with spans
curl "http://localhost:3000/api/traces/<trace-id>"

# List metrics
curl "http://localhost:3000/api/metrics?name=http_requests_total"

# Token usage (GenAI/LLM)
curl "http://localhost:3000/api/genai/usage?start_time=<ns>&end_time=<ns>"

# Health check
curl "http://localhost:3000/api/health"
```

## Development

Issues and feature requests are tracked on [GitHub Issues](https://github.com/planetf1/otelite/issues).

```bash
# Clone and build
git clone https://github.com/planetf1/otelite.git
cd otelite
cargo build --workspace

# Run tests
cargo test --workspace

# Run quality gates
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow and [docs/testing.md](docs/testing.md) for testing guide.

## Architecture

```
┌─────────────────────────────────────────────┐
│         Web Dashboard (port 3000)           │
│         + REST API (otelite-server)           │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│       SQLite Storage (otelite-storage)        │
│            with FTS5 search                 │
└─────────────────▲───────────────────────────┘
                  │
┌─────────────────┴───────────────────────────┐
│       OTLP Receivers (otelite-receiver)       │
│    gRPC (4317) + HTTP (4318)                │
└─────────────────────────────────────────────┘
```

**Crate structure:**
- `otelite-core` — Domain types (LogRecord, Span, Metric, Resource, GenAiSpanInfo)
- `otelite-storage` — SQLite backend with async trait
- `otelite-receiver` — OTLP gRPC and HTTP ingest
- `otelite-server` — REST API and web dashboard
- `otelite-cli` — Command-line interface binary
- `otelite-tui` — Terminal user interface

See [docs/architecture.md](docs/architecture.md) for detailed design.

## Performance

| Metric | Target | Typical |
|--------|--------|---------|
| Memory (idle) | <100MB | ~50MB |
| CPU (idle) | <5% | ~2% |
| Startup time | <3s | ~1.5s |
| Throughput | 1000 events/s | 2000+ events/s |

## Project Status

**Current version:** 0.1.0-alpha — in active development. API may change.

## License

Apache License 2.0 — see [LICENSE](LICENSE)
