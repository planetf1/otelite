# Rotel API Backend

Shared REST API backend providing consistent data access for all Rotel frontends (Dashboard, TUI, CLI). Serves logs, traces, and metrics from the storage backend with standardized JSON responses.

## Features

- **RESTful API** - Clean, consistent REST endpoints for all telemetry data
- **OpenAPI Documentation** - Interactive Swagger UI at `/docs`
- **Comprehensive Filtering** - Query by time range, severity, service, attributes, and more
- **Pagination Support** - Efficient offset-based pagination with metadata
- **Health Checks** - `/health` and `/ready` endpoints for monitoring and orchestration
- **CORS Support** - Configurable cross-origin resource sharing
- **Structured Logging** - Request/response logging with tracing
- **Error Handling** - Consistent error responses with proper HTTP status codes

## Quick Start

### Running the Server

```bash
# Start with default configuration (localhost:8080)
cargo run --bin rotel-api

# Start with custom configuration
ROTEL_API_HOST=0.0.0.0 ROTEL_API_PORT=3000 cargo run --bin rotel-api
```

### Configuration

The API server can be configured via environment variables or programmatically:

```rust
use rotel_api::{ApiConfig, ApiServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ApiConfig::builder()
        .host("0.0.0.0")
        .port(8080)
        .cors_enabled(true)
        .build();

    let server = ApiServer::new(config);
    server.run().await?;
    Ok(())
}
```

### Environment Variables

- `ROTEL_API_HOST` - Bind address (default: `127.0.0.1`)
- `ROTEL_API_PORT` - Port number (default: `8080`)
- `ROTEL_API_CORS_ENABLED` - Enable CORS (default: `true`)
- `RUST_LOG` - Logging level (default: `info,rotel_api=debug`)

## API Endpoints

### Logs

- `GET /api/v1/logs` - List logs with filtering and pagination
  - Query params: `severity`, `service_name`, `search`, `start_time`, `end_time`, `since`, `limit`, `offset`
- `GET /api/v1/logs/:id` - Get a specific log entry by ID

### Traces

- `GET /api/v1/traces` - List traces with filtering and pagination
  - Query params: `service_name`, `span_name`, `min_duration_ns`, `max_duration_ns`, `start_time`, `end_time`, `since`, `limit`, `offset`
- `GET /api/v1/traces/:id` - Get a specific trace with full span hierarchy

### Metrics

- `GET /api/v1/metrics` - List metrics with filtering and pagination
  - Query params: `name`, `metric_type`, `service_name`, `start_time`, `end_time`, `since`, `attributes`, `limit`, `offset`
- `GET /api/v1/metrics/:name/stats` - Get aggregated statistics for a metric
  - Returns min, max, avg, stddev, percentiles (p50, p95, p99)

### Health & Readiness

- `GET /health` - Comprehensive health check with system statistics
  - Returns service status, uptime, memory/CPU usage, component health
- `GET /ready` - Readiness check for orchestrators (Kubernetes, Docker Swarm)
  - Returns whether service is ready to accept traffic

### Documentation

- `GET /docs` - Interactive Swagger UI for API exploration
- `GET /api-docs/openapi.json` - OpenAPI 3.0 specification

## Response Format

All API responses follow a consistent structure:

### List Responses

```json
{
  "items": [...],
  "pagination": {
    "total": 100,
    "offset": 0,
    "limit": 50,
    "count": 50,
    "next_offset": 50,
    "prev_offset": null
  }
}
```

### Error Responses

```json
{
  "error": "Error message",
  "status": 400
}
```

## Filtering and Pagination

### Time Range Filtering

All endpoints support flexible time range filtering:

```bash
# Absolute time range (Unix timestamps in milliseconds)
GET /api/v1/logs?start_time=1713394800000&end_time=1713398400000

# Relative time range
GET /api/v1/logs?since=1h    # Last hour
GET /api/v1/logs?since=30m   # Last 30 minutes
GET /api/v1/logs?since=7d    # Last 7 days
```

### Pagination

```bash
# First page (50 items)
GET /api/v1/logs?limit=50&offset=0

# Second page
GET /api/v1/logs?limit=50&offset=50

# Use pagination.next_offset from response for next page
```

### Filtering Examples

```bash
# Logs by severity
GET /api/v1/logs?severity=ERROR

# Logs by service
GET /api/v1/logs?service_name=rotel-api

# Logs with text search
GET /api/v1/logs?search=database

# Traces by duration (nanoseconds)
GET /api/v1/traces?min_duration_ns=1000000

# Metrics by type
GET /api/v1/metrics?metric_type=COUNTER
```

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test logs_integration_test
cargo test --test traces_integration_test
cargo test --test metrics_integration_test
cargo test --test health_integration_test

# Run with output
cargo test -- --nocapture
```

### Building

```bash
# Development build
cargo build

# Release build
cargo build --release

# Check without building
cargo check
```

### Code Quality

```bash
# Run clippy lints
cargo clippy

# Format code
cargo fmt

# Run all quality checks
cargo clippy && cargo fmt --check && cargo test
```

## Architecture

### Project Structure

```
crates/rotel-api/
├── src/
│   ├── config.rs          # Configuration management
│   ├── error.rs           # Error types and handling
│   ├── handlers/          # Request handlers
│   │   ├── logs.rs
│   │   ├── traces.rs
│   │   ├── metrics.rs
│   │   └── health.rs
│   ├── middleware/        # HTTP middleware
│   │   ├── cors.rs
│   │   ├── logging.rs
│   │   └── error.rs
│   ├── models/            # Data models
│   │   ├── request.rs
│   │   ├── response.rs
│   │   ├── trace.rs
│   │   ├── metric.rs
│   │   ├── health.rs
│   │   └── pagination.rs
│   ├── routes/            # Route definitions
│   │   ├── logs.rs
│   │   ├── traces.rs
│   │   ├── metrics.rs
│   │   ├── health.rs
│   │   └── docs.rs
│   ├── server.rs          # HTTP server
│   └── lib.rs             # Library root
├── tests/                 # Integration tests
│   ├── logs_integration_test.rs
│   ├── traces_integration_test.rs
│   ├── metrics_integration_test.rs
│   └── health_integration_test.rs
└── Cargo.toml
```

### Technology Stack

- **Web Framework**: axum 0.7+ (built on tokio and hyper)
- **Serialization**: serde + serde_json
- **Validation**: validator (derive macros)
- **API Documentation**: utoipa + utoipa-swagger-ui (OpenAPI 3.0)
- **Error Handling**: thiserror
- **Middleware**: tower-http (CORS, compression, tracing)
- **Logging**: tracing + tracing-subscriber

### Design Principles

1. **Consistency** - All endpoints follow the same patterns and conventions
2. **Type Safety** - Strong typing with validation at API boundaries
3. **Error Handling** - Comprehensive error types with proper HTTP status codes
4. **Documentation** - OpenAPI specs generated from code annotations
5. **Testability** - Extensive unit and integration test coverage
6. **Performance** - Async/await throughout, efficient pagination
7. **Observability** - Structured logging and health checks

## Storage Backend Integration

The API currently uses mock data for development. To integrate with a real storage backend:

1. Implement the storage trait in `rotel-storage` crate
2. Update handlers to query storage instead of returning mock data
3. Add storage configuration to `ApiConfig`
4. Update health checks to verify storage connectivity

Example integration:

```rust
// In handlers/logs.rs
pub async fn list_logs(
    Query(params): Query<LogQueryParams>,
    Extension(storage): Extension<Arc<dyn Storage>>,
) -> ApiResult<Json<ListResponse<LogEntry>>> {
    let logs = storage.query_logs(params).await?;
    // ... rest of handler
}
```

## Performance Considerations

- **Pagination**: Always use pagination for large result sets
- **Filtering**: Apply filters at the storage layer when possible
- **Caching**: Consider adding caching for frequently accessed data
- **Rate Limiting**: Add rate limiting middleware for production deployments
- **Connection Pooling**: Use connection pooling for database access

## Security

- **CORS**: Configure CORS appropriately for your deployment
- **Authentication**: Add authentication middleware before production use
- **Input Validation**: All inputs are validated using the validator crate
- **Error Messages**: Error messages don't leak sensitive information

## Deployment

### Docker

```dockerfile
FROM rust:1.77 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin rotel-api

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/rotel-api /usr/local/bin/
EXPOSE 8080
CMD ["rotel-api"]
```

### Kubernetes

```yaml
apiVersion: v1
kind: Service
metadata:
  name: rotel-api
spec:
  selector:
    app: rotel-api
  ports:
    - port: 8080
      targetPort: 8080
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: rotel-api
spec:
  replicas: 3
  selector:
    matchLabels:
      app: rotel-api
  template:
    metadata:
      labels:
        app: rotel-api
    spec:
      containers:
      - name: rotel-api
        image: rotel-api:latest
        ports:
        - containerPort: 8080
        env:
        - name: ROTEL_API_HOST
          value: "0.0.0.0"
        - name: ROTEL_API_PORT
          value: "8080"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 30
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 10
```

## Contributing

1. Follow the existing code style and conventions
2. Add tests for new features
3. Update documentation for API changes
4. Run `cargo clippy` and `cargo fmt` before committing
5. Ensure all tests pass with `cargo test`

## License

Apache-2.0

## Made with Bob
