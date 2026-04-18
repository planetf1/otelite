# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## How to Start a Session

1. Run `bd ready` to see available beads sorted by priority
2. Pick the highest-priority unblocked bead
3. Run `bd show <id>` to read the full description — it has step-by-step instructions
4. Run `bd update <id> --claim` to claim it
5. Follow the instructions in the bead description precisely
6. When done, run quality gates, commit, push, and close the bead (see below)
7. Repeat with the next bead

## Commit and Push After EVERY Bead

**This is critical.** After completing each bead:

1. Run quality gates:
   ```bash
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --check
   ```
2. If any gate fails, fix the issue before committing
3. Commit with a clear message describing what was done
4. Push immediately:
   ```bash
   git push
   ```
5. Close the bead:
   ```bash
   bd close <id> --reason "what was done"
   ```
6. Push beads data:
   ```bash
   bd dolt push
   ```

**Do NOT batch multiple beads into one commit.** Each bead = one commit + push.

## Build & Test

```bash
cargo build --workspace          # build everything
cargo test --workspace           # run all tests
cargo test -p <crate-name>      # test specific crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

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

## Quality Standards

These rules apply to ALL code changes:

### Code quality rules

- **No `#[allow(dead_code)]`** unless the code is genuinely needed soon (add a comment explaining why)
- **No `unwrap()` or `expect()`** on user-facing code paths (tests are fine)
- **No silent error swallowing** — every `?` should propagate to a meaningful error message
- **No TODO comments** without a corresponding bead — if work is deferred, create a bead for it
- **Error messages must include context** — what was attempted, what failed, what to try next
- **Tests must assert specific values** — not just "doesn't panic"

## Session End

Before ending a session:

1. Ensure all work is committed and pushed
2. Close completed beads, file new beads for unfinished work
3. Run `bd dolt push` to sync beads data
4. Brief retrospective: could any rule, doc, or bead description be improved? If so, fix it or file a bead.
