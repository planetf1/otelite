# Project Instructions for AI Agents

This file provides project-specific technical context. For workflow, quality gates, and session management, see **AGENTS.md**.

## Issue Tracking

Issues are tracked on GitHub: https://github.com/planetf1/otelite/issues

```bash
gh issue list --state open --label "priority:p2"   # Find available work
gh issue view <number>                              # Read full description
gh issue comment <number> --body "..."             # Comment / claim / close
```

See AGENTS.md for full workflow rules, definition of done, and session management.

## Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo test -p <crate-name>      # test specific crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

All four must pass before every commit.

## Architecture Overview

Otelite is an OpenTelemetry receiver and local observability server for LLM developers. See docs/architecture.md for full detail.

**Crate structure:**
- `crates/otelite-core` — telemetry domain types (LogRecord, Span, Metric, Resource, GenAiSpanInfo). No HTTP/storage deps.
- `crates/otelite-receiver` — OTLP ingest: gRPC (4317) and HTTP (4318), converts protobuf → otelite-core types
- `crates/otelite-storage` — SQLite backend (WAL mode, FTS5). `StorageBackend` async trait with 10 methods. Only impl: `SqliteBackend`.
- `crates/otelite-server` — HTTP server (port 3000): REST API + embedded static web UI.
- `crates/otelite-cli` — clap CLI binary: `serve` (starts everything), `logs`/`traces`/`metrics` (query subcommands)
- `crates/otelite-tui` — ratatui terminal UI, polls otelite-server REST API

**Data flow:** OTLP Source → Receiver → Storage (SQLite) → otelite-server REST API → CLI / TUI / Web browser

**Known gotchas for agents:**
- `Span` has no `links` field
- `LogRecord` has `observed_timestamp: Option<i64>`
- No MockStorage exists — tests use real `SqliteBackend` with `tempfile::TempDir`

## Conventions & Patterns

- Rust 1.77+ stable
- Async via tokio; axum for HTTP; tonic for gRPC
- `thiserror` for error types; no silent `?` swallowing
- No agent attribution in commits — do not add Co-Authored-By, Assisted-by, or similar trailers

## Code Quality Rules

- **No `#[allow(dead_code)]`** unless genuinely needed soon (add a comment explaining why)
- **No `unwrap()` or `expect()`** on user-facing code paths (tests are fine)
- **No silent error swallowing** — every `?` should propagate to a meaningful error message
- **No TODO comments** without a corresponding GitHub issue
- **Error messages must include context** — what was attempted, what failed, what to try next
- **Tests must assert specific values** — not just "doesn't panic"
