# Rotel Architecture

This document describes the high-level architecture, design decisions, and component interactions in Rotel.

## Table of Contents

- [Overview](#overview)
- [Design Principles](#design-principles)
- [System Architecture](#system-architecture)
- [Component Details](#component-details)
- [Data Flow](#data-flow)
- [Storage Architecture](#storage-architecture)
- [Plugin System](#plugin-system)
- [Performance Considerations](#performance-considerations)
- [Security](#security)
- [Future Enhancements](#future-enhancements)

## Overview

Rotel is a lightweight OpenTelemetry receiver and dashboard designed for local LLM users. It provides a complete observability solution with minimal resource footprint, supporting metrics, logs, and traces through standard OTLP protocols.

### Key Goals

1. **Lightweight**: Minimal memory (<100MB) and CPU (<5%) usage
2. **Standards Compliant**: Full OTLP protocol support
3. **Easy to Use**: Single binary, zero configuration required
4. **Extensible**: Pluggable architecture for customization
5. **Cross-Platform**: Support for macOS and Linux

## Design Principles

### 1. Simplicity First

- Single binary deployment
- Embedded storage by default (no external dependencies)
- Sensible defaults requiring zero configuration
- Clear error messages with actionable guidance

### 2. Performance by Design

- Async I/O throughout (Tokio runtime)
- Zero-copy parsing where possible
- Efficient data structures (arena allocation, pooling)
- Lazy evaluation and streaming processing

### 3. Standards Compliance

- Full OTLP/gRPC and OTLP/HTTP support
- OpenTelemetry semantic conventions
- Interoperability with standard exporters
- No proprietary extensions required

### 4. Extensibility

- Plugin interfaces for storage, parsing, and export
- Well-defined internal APIs
- Stable plugin ABI with semantic versioning
- Core functionality independent of plugins

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Dashboard UI                            │
│                   (Web Interface)                            │
└────────────────────────┬────────────────────────────────────┘
                         │ HTTP/WebSocket
                         │
┌────────────────────────▼────────────────────────────────────┐
│                   Query Engine                               │
│  - Query Parser                                              │
│  - Query Optimizer                                           │
│  - Result Formatter                                          │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                 Storage Backend                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Embedded   │  │  PostgreSQL  │  │   Plugin     │      │
│  │   (Default)  │  │  (Optional)  │  │  (Custom)    │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└────────────────────────▲────────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────────┐
│                  Data Pipeline                               │
│  - Validation                                                │
│  - Transformation                                            │
│  - Batching                                                  │
│  - Compression                                               │
└────────────────────────▲────────────────────────────────────┘
                         │
┌────────────────────────┴────────────────────────────────────┐
│                  OTLP Receiver                               │
│  ┌──────────────┐              ┌──────────────┐            │
│  │ OTLP/gRPC    │              │ OTLP/HTTP    │            │
│  │ (Port 4317)  │              │ (Port 4318)  │            │
│  └──────────────┘              └──────────────┘            │
└─────────────────────────────────────────────────────────────┘
```

## Component Details

### OTLP Receiver

**Purpose**: Accept telemetry data via OTLP protocol

**Technology**:
- gRPC: Tonic (high-performance gRPC implementation)
- HTTP: Axum (ergonomic web framework)
- Protocol: OpenTelemetry Protocol (OTLP) v1.0+

**Responsibilities**:
- Accept OTLP/gRPC and OTLP/HTTP requests
- Validate protocol compliance
- Parse protobuf messages
- Forward to data pipeline

**Key Files**:
- `crates/rotel-receiver/src/grpc.rs` - gRPC endpoint
- `crates/rotel-receiver/src/http.rs` - HTTP endpoint
- `crates/rotel-receiver/src/validation.rs` - Protocol validation

**Configuration**:
```toml
[receiver]
grpc_port = 4317
http_port = 4318
max_message_size_mb = 4
compression = ["gzip", "zstd"]
```

### Data Pipeline

**Purpose**: Transform and prepare data for storage

**Stages**:
1. **Validation**: Verify data integrity and schema compliance
2. **Transformation**: Normalize and enrich data
3. **Batching**: Group data for efficient storage
4. **Compression**: Reduce storage footprint

**Technology**:
- Async processing with Tokio channels
- Backpressure handling with bounded channels
- Batch processing with configurable windows

**Key Files**:
- `crates/rotel-pipeline/src/validator.rs` - Data validation
- `crates/rotel-pipeline/src/transformer.rs` - Data transformation
- `crates/rotel-pipeline/src/batcher.rs` - Batch processing

**Configuration**:
```toml
[pipeline]
batch_size = 1000
batch_timeout_ms = 1000
max_queue_size = 10000
compression = "zstd"
```

### Storage Backend

**Purpose**: Persist and retrieve telemetry data

**Embedded Storage (Default)**:
- Technology: RocksDB or sled (TBD)
- Features: Embedded, zero-config, efficient indexing
- Limitations: Single-node only, ~10GB recommended max

**PostgreSQL Backend (Optional)**:
- Technology: PostgreSQL 14+ with TimescaleDB extension
- Features: Distributed, scalable, SQL queries
- Use case: Production deployments, large datasets

**Plugin Interface**:
```rust
pub trait StorageBackend: Send + Sync {
    async fn write_metrics(&self, metrics: Vec<Metric>) -> Result<()>;
    async fn write_logs(&self, logs: Vec<LogRecord>) -> Result<()>;
    async fn write_traces(&self, traces: Vec<Span>) -> Result<()>;

    async fn query_metrics(&self, query: MetricQuery) -> Result<Vec<Metric>>;
    async fn query_logs(&self, query: LogQuery) -> Result<Vec<LogRecord>>;
    async fn query_traces(&self, query: TraceQuery) -> Result<Vec<Span>>;
}
```

**Key Files**:
- `crates/rotel-storage/src/trait.rs` - Storage trait
- `crates/rotel-storage/src/embedded.rs` - Embedded backend
- `crates/rotel-storage/src/postgres.rs` - PostgreSQL backend

### Query Engine

**Purpose**: Execute queries against stored data

**Features**:
- Time-range queries
- Attribute filtering
- Aggregations (sum, avg, count, percentiles)
- Trace correlation
- Metric downsampling

**Query Language** (Future):
```
metrics{service="api", status="200"} | rate(5m) | avg()
logs{level="error"} | count() by service
traces{duration > 1s} | histogram(duration)
```

**Key Files**:
- `crates/rotel-query/src/parser.rs` - Query parsing
- `crates/rotel-query/src/optimizer.rs` - Query optimization
- `crates/rotel-query/src/executor.rs` - Query execution

### Dashboard UI

**Purpose**: Visualize telemetry data

**Technology**:
- Backend: Axum (REST API + WebSocket)
- Frontend: TBD (React/Vue/Svelte)
- Charts: TBD (Chart.js/D3.js)

**Features**:
- Real-time metrics visualization
- Log search and filtering
- Trace timeline view
- Service dependency graph
- Custom dashboards

**Key Files**:
- `crates/rotel-dashboard/src/api.rs` - REST API
- `crates/rotel-dashboard/src/websocket.rs` - Real-time updates
- `crates/rotel-dashboard/static/` - Frontend assets

## Data Flow

### Ingestion Flow

```
1. Client sends OTLP data
   ↓
2. Receiver validates protocol
   ↓
3. Pipeline validates data
   ↓
4. Pipeline transforms/enriches
   ↓
5. Pipeline batches data
   ↓
6. Storage backend persists
   ↓
7. Acknowledgment sent to client
```

### Query Flow

```
1. User submits query (UI/API)
   ↓
2. Query engine parses query
   ↓
3. Query optimizer plans execution
   ↓
4. Storage backend executes query
   ↓
5. Query engine formats results
   ↓
6. Results returned to user
```

## Storage Architecture

### Data Model

**Metrics**:
```rust
struct Metric {
    name: String,
    timestamp: i64,
    value: f64,
    attributes: HashMap<String, String>,
    resource: Resource,
}
```

**Logs**:
```rust
struct LogRecord {
    timestamp: i64,
    severity: Severity,
    body: String,
    attributes: HashMap<String, String>,
    resource: Resource,
    trace_id: Option<TraceId>,
    span_id: Option<SpanId>,
}
```

**Traces**:
```rust
struct Span {
    trace_id: TraceId,
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    name: String,
    start_time: i64,
    end_time: i64,
    attributes: HashMap<String, String>,
    events: Vec<Event>,
    links: Vec<Link>,
    status: Status,
}
```

### Indexing Strategy

**Time-based Indexing**:
- Primary index on timestamp
- Partitioning by time range (daily/hourly)
- Automatic retention and compaction

**Attribute Indexing**:
- Secondary indexes on common attributes
- Inverted index for text search
- Bloom filters for existence checks

**Trace Indexing**:
- Index on trace_id for trace assembly
- Index on span_id for parent-child relationships
- Index on duration for latency queries

## Plugin System

### Plugin Types

1. **Storage Plugins**: Custom storage backends
2. **Parser Plugins**: Custom data formats
3. **Exporter Plugins**: Forward data to external systems
4. **Transformer Plugins**: Custom data transformations

### Plugin Interface

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn init(&mut self, config: &Config) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
}
```

### Plugin Loading

- Dynamic loading via `libloading`
- Plugin discovery in `~/.rotel/plugins/`
- Version compatibility checking
- Graceful degradation on plugin failure

## Performance Considerations

### Memory Management

- **Arena Allocation**: Batch allocations for related data
- **Object Pooling**: Reuse buffers and objects
- **Zero-Copy**: Avoid unnecessary data copies
- **Streaming**: Process data in chunks, not all at once

### Concurrency

- **Async I/O**: Non-blocking operations throughout
- **Work Stealing**: Tokio's work-stealing scheduler
- **Bounded Channels**: Backpressure handling
- **Lock-Free Structures**: Where possible (e.g., metrics counters)

### Optimization Techniques

- **Batch Processing**: Group operations for efficiency
- **Compression**: Reduce storage and network overhead
- **Caching**: Query result caching with TTL
- **Lazy Evaluation**: Defer work until needed

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Memory (idle) | <100MB | RSS |
| CPU (idle) | <5% | Average over 1 minute |
| Throughput | 1000 events/s | On commodity hardware |
| Latency (p99) | <100ms | End-to-end ingestion |
| Startup time | <3s | From launch to ready |

## Security

### Authentication & Authorization

- API key authentication (optional)
- Role-based access control (RBAC)
- TLS/SSL for all network communication
- Token-based session management

### Data Protection

- Encryption at rest (optional)
- Encryption in transit (TLS)
- Secret detection in CI/CD
- No secrets in logs or error messages

### Input Validation

- Protocol validation (OTLP compliance)
- Schema validation (protobuf)
- Size limits (max message size)
- Rate limiting (per-client quotas)

## Future Enhancements

### Planned Features

1. **Query Language**: Advanced filtering and aggregation
2. **Alerting**: Threshold-based alerts and notifications
3. **Distributed Tracing**: Enhanced trace visualization
4. **Metrics Aggregation**: Downsampling and rollups
5. **Plugin Marketplace**: Community-contributed plugins

### Scalability Roadmap

1. **Horizontal Scaling**: Multi-node deployment
2. **Sharding**: Distribute data across nodes
3. **Replication**: High availability and fault tolerance
4. **Federation**: Connect multiple Rotel instances

### Integration Roadmap

1. **Prometheus**: Metrics export to Prometheus
2. **Grafana**: Grafana data source plugin
3. **Jaeger**: Trace export to Jaeger
4. **Elasticsearch**: Log export to Elasticsearch

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on contributing to Rotel's architecture.

For questions or discussions about architecture decisions, please open a [GitHub Discussion](https://github.com/YOUR_USERNAME/rotel/discussions).

---

**Last Updated**: 2026-04-17  
**Version**: 0.1.0-alpha
