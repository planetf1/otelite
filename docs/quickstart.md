# Rotel Quick Start Guide

Get up and running with Rotel in 5 minutes.

## Prerequisites

- **Operating System**: macOS or Linux
- **Rust**: 1.77+ (for building from source)
- **Git**: For cloning the repository

## Installation

### Option 1: Pre-built Binary (Recommended)

**macOS (Homebrew)**:
```bash
brew install rotel
```

**Linux (x86_64)**:
```bash
curl -L https://github.com/YOUR_USERNAME/rotel/releases/latest/download/rotel-linux-x86_64.tar.gz | tar xz
sudo mv rotel /usr/local/bin/
```

**Linux (ARM64)**:
```bash
curl -L https://github.com/YOUR_USERNAME/rotel/releases/latest/download/rotel-linux-arm64.tar.gz | tar xz
sudo mv rotel /usr/local/bin/
```

### Option 2: Build from Source

```bash
# Clone repository
git clone https://github.com/YOUR_USERNAME/rotel.git
cd rotel

# Build release binary
cargo build --release

# Install to system
sudo cp target/release/rotel /usr/local/bin/
```

### Verify Installation

```bash
rotel --version
# Output: rotel 0.1.0-alpha
```

## Starting Rotel

### Basic Usage

Start Rotel with default configuration:

```bash
rotel start
```

This starts:
- **OTLP/gRPC** receiver on `localhost:4317`
- **OTLP/HTTP** receiver on `localhost:4318`
- **Dashboard** on `http://localhost:8080`

### Custom Configuration

Create a configuration file `rotel.toml`:

```toml
[server]
grpc_port = 4317
http_port = 4318
dashboard_port = 8080

[storage]
backend = "embedded"
data_dir = "~/.rotel/data"
max_size_gb = 10

[limits]
max_events_per_second = 1000
max_memory_mb = 100
```

Start with custom config:

```bash
rotel start --config rotel.toml
```

### Running as a Service

**systemd (Linux)**:

Create `/etc/systemd/system/rotel.service`:

```ini
[Unit]
Description=Rotel OpenTelemetry Receiver
After=network.target

[Service]
Type=simple
User=rotel
ExecStart=/usr/local/bin/rotel start --config /etc/rotel/rotel.toml
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl enable rotel
sudo systemctl start rotel
sudo systemctl status rotel
```

**launchd (macOS)**:

Create `~/Library/LaunchAgents/com.rotel.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.rotel</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/rotel</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

Load and start:

```bash
launchctl load ~/Library/LaunchAgents/com.rotel.plist
launchctl start com.rotel
```

## Instrumenting Your Application

### Rust

Add dependencies to `Cargo.toml`:

```toml
[dependencies]
opentelemetry = "0.21"
opentelemetry-otlp = "0.14"
opentelemetry_sdk = "0.21"
tokio = { version = "1", features = ["full"] }
```

Configure OTLP exporter:

```rust
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::TracerProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracer
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317")
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    global::set_tracer_provider(tracer);

    // Use tracer
    let tracer = global::tracer("my-app");
    tracer.in_span("my-operation", |_cx| {
        // Your code here
    });

    Ok(())
}
```

### Python

Install dependencies:

```bash
pip install opentelemetry-api opentelemetry-sdk opentelemetry-exporter-otlp
```

Configure OTLP exporter:

```python
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

# Initialize tracer
trace.set_tracer_provider(TracerProvider())
otlp_exporter = OTLPSpanExporter(endpoint="http://localhost:4317")
trace.get_tracer_provider().add_span_processor(
    BatchSpanProcessor(otlp_exporter)
)

# Use tracer
tracer = trace.get_tracer(__name__)
with tracer.start_as_current_span("my-operation"):
    # Your code here
    pass
```

### JavaScript/Node.js

Install dependencies:

```bash
npm install @opentelemetry/api @opentelemetry/sdk-node @opentelemetry/exporter-trace-otlp-grpc
```

Configure OTLP exporter:

```javascript
const { NodeSDK } = require('@opentelemetry/sdk-node');
const { OTLPTraceExporter } = require('@opentelemetry/exporter-trace-otlp-grpc');

const sdk = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: 'http://localhost:4317',
  }),
});

sdk.start();

// Use tracer
const { trace } = require('@opentelemetry/api');
const tracer = trace.getTracer('my-app');

tracer.startActiveSpan('my-operation', (span) => {
  // Your code here
  span.end();
});
```

### Go

Install dependencies:

```bash
go get go.opentelemetry.io/otel
go get go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc
go get go.opentelemetry.io/otel/sdk/trace
```

Configure OTLP exporter:

```go
package main

import (
    "context"
    "go.opentelemetry.io/otel"
    "go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracegrpc"
    "go.opentelemetry.io/otel/sdk/trace"
)

func main() {
    ctx := context.Background()

    // Initialize tracer
    exporter, _ := otlptracegrpc.New(ctx,
        otlptracegrpc.WithEndpoint("localhost:4317"),
        otlptracegrpc.WithInsecure(),
    )

    tp := trace.NewTracerProvider(
        trace.WithBatcher(exporter),
    )
    otel.SetTracerProvider(tp)

    // Use tracer
    tracer := otel.Tracer("my-app")
    _, span := tracer.Start(ctx, "my-operation")
    defer span.End()

    // Your code here
}
```

## Viewing Data

### Dashboard

Open your browser and navigate to:

```
http://localhost:8080
```

The dashboard provides:
- **Metrics**: Time-series charts and aggregations
- **Logs**: Search, filter, and view log entries
- **Traces**: Distributed trace timeline and service graph

### CLI Queries

Query data using the CLI:

```bash
# View recent metrics
rotel query metrics --last 1h

# Search logs
rotel query logs --filter 'level=error' --last 30m

# View traces
rotel query traces --min-duration 1s --last 1h
```

## Next Steps

- **Configuration**: Learn about [configuration options](configuration.md)
- **Testing**: Set up [testing infrastructure](testing.md)
- **Troubleshooting**: Check [common issues](troubleshooting.md)
- **Contributing**: Read the [contributing guide](../CONTRIBUTING.md)

## Common Issues

### Port Already in Use

If ports 4317, 4318, or 8080 are already in use:

```bash
# Check what's using the port
lsof -i :4317

# Start Rotel with different ports
rotel start --grpc-port 14317 --http-port 14318 --dashboard-port 18080
```

### Permission Denied

If you get permission errors:

```bash
# Create data directory with correct permissions
mkdir -p ~/.rotel/data
chmod 755 ~/.rotel/data

# Or run with sudo (not recommended)
sudo rotel start
```

### Connection Refused

If your application can't connect to Rotel:

1. Verify Rotel is running: `ps aux | grep rotel`
2. Check firewall settings
3. Verify endpoint URL in your application
4. Check Rotel logs: `rotel logs`

## Getting Help

- **Documentation**: Check the [docs](.) directory
- **Issues**: Report bugs on [GitHub Issues](https://github.com/YOUR_USERNAME/rotel/issues)
- **Discussions**: Ask questions on [GitHub Discussions](https://github.com/YOUR_USERNAME/rotel/discussions)

---

**Next**: [Configuration Guide](configuration.md) | [Testing Guide](testing.md)
