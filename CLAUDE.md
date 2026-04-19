# Project Instructions for AI Agents

This file provides project-specific technical context. For workflow, quality gates, and session management, see **AGENTS.md**.

## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking.

```bash
bd ready              # Find available work (sorted by priority)
bd ready -l pipeline  # Filter by label
bd show <id>          # Read full description before starting
bd update <id> --claim  # Claim work
bd close <id> --reason "what was done"  # Complete work
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

Rotel is an OpenTelemetry receiver and local observability server for LLM developers. See ARCHITECTURE.md for full detail.

**Crate structure:**
- `crates/rotel-core` — telemetry domain types (LogRecord, Span, Metric, Resource, GenAiSpanInfo). No HTTP/storage deps.
- `crates/rotel-receiver` — OTLP ingest: gRPC (4317) and HTTP (4318), converts protobuf → rotel-core types
- `crates/rotel-storage` — SQLite backend (WAL mode, FTS5). `StorageBackend` async trait with 10 methods. Only impl: `SqliteBackend`.
- `crates/rotel-dashboard` — HTTP server (port 3000): REST API + embedded static web UI. **Note: misnamed — it's a server, not just a dashboard. Rename tracked in bead rotel-jfa.**
- `crates/rotel-cli` — clap CLI binary: `dashboard` (starts everything), `logs`/`traces`/`metrics` (query subcommands)
- `crates/rotel-tui` — ratatui terminal UI, polls rotel-dashboard REST API

**Data flow:** OTLP Source → Receiver → Storage (SQLite) → rotel-dashboard REST API → CLI / TUI / Web browser

**Known gotchas for agents:**
- `Span` has no `links` field
- `LogRecord` has `observed_timestamp: Option<i64>`
- CLI default endpoint is `localhost:8080` but server binds `:3000` (bug: rotel-2h2)
- API response types (`LogEntry`, `TraceEntry`, etc.) are duplicated in dashboard/CLI/TUI (debt: rotel-d9q)
- `rotel-core/src/lib.rs` contains scaffolding `add()`/`divide()`/`Config` to be removed (rotel-y90)
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
- **No TODO comments** without a corresponding bead — if work is deferred, create a bead for it
- **Error messages must include context** — what was attempted, what failed, what to try next
- **Tests must assert specific values** — not just "doesn't panic"
