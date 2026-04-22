# Otelite Quick Start Guide

Get up and running with Otelite in 5 minutes.

## Prerequisites

### Install Rust

If you don't have Rust installed, use [rustup](https://rustup.rs/):

```bash
# macOS and Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Otelite requires Rust stable (1.77+). `rustup` installs the latest stable by default.

## Installation

### Build from Source

```bash
# Clone repository
git clone https://github.com/planetf1/otelite.git
cd otelite

# Build and install
cargo install --path crates/otelite-cli
```

Verify installation:

```bash
otelite --version
```

## First Run

Start the Otelite server:

```bash
otelite serve
```

This starts:
- **OTLP/gRPC** receiver on `localhost:4317`
- **OTLP/HTTP** receiver on `localhost:4318`
- **REST API** on `http://localhost:3000`
- **Web Dashboard** on `http://localhost:3000`

You should see output like:

```
Otelite starting...
OTLP gRPC receiver listening on 0.0.0.0:4317
OTLP HTTP receiver listening on 0.0.0.0:4318
REST API listening on http://0.0.0.0:3000
Web dashboard available at http://localhost:3000
```

## Sending Test Data

### Using otel-cli

Install [otel-cli](https://github.com/equinix-labs/otel-cli):

```bash
# macOS
brew install otel-cli

# Linux
curl -L https://github.com/equinix-labs/otel-cli/releases/latest/download/otel-cli-linux-amd64 -o otel-cli
chmod +x otel-cli
sudo mv otel-cli /usr/local/bin/
```

Send a test trace:

```bash
otel-cli span --endpoint localhost:4317 \
  --service my-service \
  --name "test-operation" \
  --attrs "key1=value1,key2=value2"
```

### Using curl

Send a test log via HTTP:

```bash
curl -X POST http://localhost:4318/v1/logs \
  -H "Content-Type: application/json" \
  -d '{
    "resourceLogs": [{
      "resource": {
        "attributes": [{
          "key": "service.name",
          "value": {"stringValue": "my-service"}
        }]
      },
      "scopeLogs": [{
        "logRecords": [{
          "timeUnixNano": "1609459200000000000",
          "severityText": "INFO",
          "body": {"stringValue": "Test log message"}
        }]
      }]
    }]
  }'
```

### Using Python SDK

Install dependencies:

```bash
pip install opentelemetry-api opentelemetry-sdk opentelemetry-exporter-otlp
```

Send a test trace:

```python
from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

# Configure
trace.set_tracer_provider(TracerProvider())
otlp_exporter = OTLPSpanExporter(endpoint="http://localhost:4317", insecure=True)
trace.get_tracer_provider().add_span_processor(BatchSpanProcessor(otlp_exporter))

# Send trace
tracer = trace.get_tracer(__name__)
with tracer.start_as_current_span("test-operation"):
    print("Sending test trace to Otelite")
```

## Viewing Data

### Web Dashboard

Open your browser and navigate to:

```
http://localhost:3000
```

The dashboard shows:
- **Logs**: Recent log entries with severity filtering
- **Traces**: Distributed traces with span details
- **Metrics**: Time-series data and aggregations

### CLI Queries

Query data using the CLI:

```bash
# List recent logs
otelite logs list

# List traces
otelite traces list

# List metrics
otelite metrics list
```

## Next Steps

- **CLI Reference**: See [CLI documentation](../crates/otelite-cli/README.md) for all commands
- **TUI Guide**: Learn about the [terminal UI](tui-quickstart.md)
- **Configuration**: Customize [configuration options](../ARCHITECTURE.md#configuration)
- **Testing**: Set up [testing infrastructure](testing.md)

## Troubleshooting

### Port Already in Use

If ports are already in use, check what's using them:

```bash
# macOS/Linux
lsof -i :4317
lsof -i :4318
lsof -i :3000
```

### Connection Refused

If your application can't connect:

1. Verify Otelite is running: `ps aux | grep otelite`
2. Check the endpoint URL matches: `http://localhost:4317` (gRPC) or `http://localhost:4318` (HTTP)
3. Check firewall settings

### No Data Appearing

If you send data but don't see it:

1. Check Otelite logs for errors
2. Verify the OTLP endpoint is correct
3. Ensure your application is using the correct protocol (gRPC vs HTTP)
4. Try the curl example above to verify the receiver is working

## Getting Help

- **Documentation**: Check the [docs](.) directory
- **Issues**: Report bugs on [GitHub Issues](https://github.com/planetf1/otelite/issues)
- **Discussions**: Ask questions on [GitHub Discussions](https://github.com/planetf1/otelite/discussions)
