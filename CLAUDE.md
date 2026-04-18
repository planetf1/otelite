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

Rotel is an OpenTelemetry receiver and dashboard for local LLM users.

**Crate structure:**
- `crates/rotel-core` — telemetry data types (LogRecord, Span, Metric, Resource)
- `crates/rotel-receiver` — OTLP gRPC (4317) and HTTP (4318) receiver
- `crates/rotel-storage` — embedded SQLite backend (WAL mode, FTS5)
- `crates/rotel-dashboard` — axum web dashboard with REST API (port 3000)
- `crates/rotel-cli` — CLI with logs/traces/metrics/dashboard subcommands
- `crates/rotel-tui` — ratatui terminal UI

**Data flow:** OTLP Source -> Receiver -> Storage (SQLite) -> Dashboard API -> CLI/TUI/Web

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
