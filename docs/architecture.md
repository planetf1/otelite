# Otelite Architecture

This document describes the actual architecture, design decisions, and component interactions in Otelite.

**Last Updated**: 2026-04-23

---

## Overview

Otelite is a lightweight OpenTelemetry receiver and local observability server for LLM developers. It accepts telemetry via OTLP (gRPC and HTTP), stores it in embedded SQLite, and exposes it via a REST API consumed by the CLI, TUI, and web interface.

Single binary, zero external dependencies, embedded storage.

---

## Crate Structure

```
otelite-core        ← telemetry domain types (no deps on other crates)
otelite-storage     ← SQLite persistence (depends on: otelite-core)
otelite-receiver    ← OTLP ingest (depends on: otelite-core, otelite-storage)
otelite-api         ← HTTP server: REST API + static web UI (depends on: otelite-core, otelite-storage)
otelite-client      ← HTTP client for the REST API (depends on: otelite-core)
otelite             ← CLI binary (depends on: otelite-core, otelite-api, otelite-client, otelite-receiver, otelite-storage)
otelite-tui         ← ratatui terminal UI (depends on: otelite-core, otelite-client)
```



The `otelite` binary is the integration point: it wires receiver + api + storage into one process for the `serve` subcommand.

---

## Data Flow

```
LLM / OpenTelemetry SDK
        │ OTLP/gRPC (4317) or OTLP/HTTP (4318)
        ▼
otelite-receiver
  - Validates OTLP protobuf
  - Converts to otelite-core types (LogRecord, Span, Metric)
  - Writes via StorageBackend trait
        │
        ▼
otelite-storage (SQLite, WAL mode, FTS5)
  - Tables: logs, spans, metrics
  - Full-text search on log body via FTS5
  - Configurable retention + auto-purge
        │
        ▼
otelite-api (HTTP server, port 3000)
  - REST API: /api/logs, /api/traces, /api/metrics (+ export, aggregate endpoints)
  - Converts storage types to JSON response types
  - LRU query cache (100 entries, 5-min TTL)
  - Serves embedded static web UI (HTML/CSS/JS)
        │
   ┌────┴─────┐
   ▼          ▼
otelite     otelite-tui
(CLI)       (ratatui terminal UI)
```

---

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 4317 | gRPC     | OTLP telemetry ingest |
| 4318 | HTTP     | OTLP telemetry ingest |
| 3000 | HTTP     | REST API + web UI |

---

## Component Details

### otelite-core

**Path:** `crates/otelite-core/`

Canonical domain types shared across all crates. No HTTP, no storage deps — pure data.

Key types:
- `LogRecord` — timestamp, severity (Trace/Debug/Info/Warn/Error/Fatal), body, attributes, resource, optional trace/span correlation
- `Span` — trace_id, span_id, parent_span_id, name, kind, start_time, end_time, attributes, events, status. **No `links` field.**
- `Metric` — name, MetricType (Gauge/Counter/Histogram/Summary), timestamp, attributes, resource
- `Resource` — `HashMap<String, String>` wrapper with `service.name` helpers
- `GenAiSpanInfo` — detects and extracts `gen_ai.*` attributes from spans for LLM observability

> **Known issue:** `lib.rs` contains scaffolding functions `add()`, `divide()`, and a `Config` struct that are not real functionality and should be removed (bead otelite-y90).

---

### otelite-storage

**Path:** `crates/otelite-storage/`

Embedded SQLite storage with async trait abstraction.

**`StorageBackend` trait** (`src/lib.rs` lines 73-105):
```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn initialize(&mut self) -> Result<()>;    // &mut self
    async fn write_log(&self, log: &LogRecord) -> Result<()>;
    async fn write_span(&self, span: &Span) -> Result<()>;
    async fn write_metric(&self, metric: &Metric) -> Result<()>;
    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>>;
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>>;
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;
    async fn stats(&self) -> Result<StorageStats>;
    async fn purge(&self, options: &PurgeOptions) -> Result<u64>;
    async fn close(&mut self) -> Result<()>;         // &mut self
}
```

Only implementation: `SqliteBackend` (`src/sqlite/mod.rs`). Uses WAL mode, FTS5 for log full-text search, `tempfile::TempDir` for test databases.

**There is no mock implementation.** Tests use `SqliteBackend` with an in-memory or temp-dir database.

**Data Retention:** By default, otelite retains 90 days of telemetry data. The retention window is configurable via the `OTELITE_RETENTION_DAYS` environment variable (set to `0` to disable automatic purging). A background task runs daily at 02:00 local time and deletes records older than the retention threshold in batches of 10,000 rows to avoid locking the database. Users can also trigger an immediate full purge via `POST /api/admin/purge` or the "Clear all data" button in the web UI status popover.

---

### otelite-receiver

**Path:** `crates/otelite-receiver/`

OTLP ingest layer. Accepts telemetry, converts protobuf to otelite-core types, writes to storage.

- `GrpcServer` — tonic-based, handles `ExportLogsService`, `ExportTracesService`, `ExportMetricsService`
- `HttpServer` — axum-based, same signals via OTLP/HTTP JSON or protobuf
- `conversion.rs` — converts `opentelemetry_proto` types to `otelite_core::telemetry::*`

Does not depend on `otelite-server` — decoupled from the query layer.

---

### otelite-api

**Path:** `crates/otelite-api/`

HTTP server exposing the REST API and serving the embedded web UI.

**`AppState`**: `Arc<dyn StorageBackend>` + `QueryCache` (LRU cache, 100 entries, 5-min TTL).

**Routes** (all GET):

| Endpoint | Handler |
|----------|---------|
| `/api/health` | health check, no storage |
| `/api/logs` | list logs with filters (severity, resource, search, time range, limit/offset) |
| `/api/logs/:timestamp` | get single log by timestamp |
| `/api/logs/export` | export logs as JSON or CSV |
| `/api/traces` | list traces with filters |
| `/api/traces/:trace_id` | get full trace with all spans |
| `/api/traces/export` | export traces as JSON |
| `/api/metrics` | list metrics with filters |
| `/api/metrics/names` | list unique metric names |
| `/api/metrics/aggregate` | aggregate metrics (sum/avg/count/min/max, optional time bucketing) |
| `/api/metrics/export` | export metrics as JSON or CSV |
| fallback | serve embedded static files |

**API response types** are defined in `src/api/{logs,traces,metrics}.rs`. These types are duplicated in `otelite/src/api/models.rs` and `otelite-tui/src/api/models.rs` — a known technical debt (bead otelite-d9q).

> **Missing derives:** `LogsResponse`, `LogEntry`, `TracesResponse`, `TraceEntry`, `TraceDetail`, `SpanEntry`, `SpanStatus`, `SpanEvent` currently only derive `Serialize`, not `Deserialize`. This blocks test writing (bead otelite-6e8).

---

### otelite

**Path:** `crates/otelite/`

clap-based CLI binary. Subcommands:

| Command | Action |
|---------|--------|
| `otelite serve` | Start full server (receiver + REST API + web UI). This is the default subcommand. |
| `otelite logs list` | List logs with filters |
| `otelite logs search <query>` | Full-text search across logs |
| `otelite logs show <timestamp>` | Show single log by timestamp |
| `otelite logs export` | Export logs |
| `otelite traces list` | List traces |
| `otelite traces show <trace_id>` | Show full trace with spans |
| `otelite traces export` | Export traces |
| `otelite metrics list` | List metrics |
| `otelite metrics show <name>` | Show metric by name |
| `otelite metrics export` | Export metrics |

Global flags: `--endpoint` (default: `http://localhost:3000`), `--format` (pretty/json), `--no-color`, `--no-header`, `--timeout`.

> **Bug:** Default endpoint is currently hardcoded to `localhost:8080` in `src/config.rs` but the server binds to `:3000`. See bead otelite-2h2.

`ApiClient` (`src/api/client.rs`) wraps `reqwest::Client`. Query params are passed as `Vec<(&str, String)>` — less type-safe than the TUI's approach.

---

### otelite-tui

**Path:** `crates/otelite-tui/`

ratatui-based terminal UI. Standalone binary, connects to otelite-api REST API via HTTP.

Default endpoint: `http://localhost:3000`.

Views: Logs / Traces / Metrics / Help. Auto-refreshes on configurable interval.

Maintains its own copy of API response types in `src/api/models.rs` with different names from the dashboard types:
- Dashboard: `TraceEntry`, `TraceDetail`, `SpanEntry`, `MetricResponse`
- TUI: `TraceSummary`, `Trace`, `Span`, `Metric`

This divergence is tracked in bead otelite-d9q.

---

## Known Technical Debt

| Issue | Bead | Priority |
|-------|------|----------|
| API response types duplicated in dashboard/CLI/TUI | otelite-d9q | P2 |

| CLI default endpoint is `:8080`, server binds `:3000` | otelite-2h2 | P1 (bug) |
| `otelite-core` contains scaffolding functions (add, divide, Config) | otelite-y90 | P2 |
| ARCHITECTURE.md was outdated (now fixed) | otelite-nyg | P2 |
| Dashboard API response types missing `Deserialize` | otelite-6e8 | P1 |
| TUI type names diverge from dashboard/CLI names | otelite-d9q | P2 |

---

## Testing Patterns

### Storage tests
Use `SqliteBackend` with `tempfile::TempDir`:
```rust
let tmp = TempDir::new().unwrap();
let config = StorageConfig::default().with_data_dir(tmp.path().to_path_buf());
let mut storage = SqliteBackend::new(config);
storage.initialize().await.unwrap();
let storage: Arc<dyn StorageBackend> = Arc::new(storage);
// tmp must stay alive for the test duration
```

See `crates/otelite-receiver/tests/pipeline_integration_test.rs` lines 21-36 for the pattern.

Test data helpers (create_test_log, create_test_span, create_test_metric) are in `crates/otelite-storage/tests/integration/persistence_test.rs` lines 264-348.

### Dashboard API tests
Use `tower::ServiceExt::oneshot()` to test the axum router without a TCP listener. Requires:
- `tower = { version = "0.5", features = ["util"] }` in dev-dependencies
- `http-body-util` for reading response bodies

See bead otelite-9mx for full test implementation guidance.

### Receiver tests
`crates/otelite-receiver/tests/` has integration tests for gRPC and HTTP ingest, with test data builders in `grpc_test_utils.rs` and `http_test_utils.rs`.
