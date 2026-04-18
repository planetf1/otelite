# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
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

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->

## Development Workflow: BobKit is LEGACY

> **IMPORTANT**: The BobKit spec-driven workflow (`.bob/commands/bobkit.*`, `.specify/` templates, `/bobkit.*` slash commands) is **legacy**. Do **not** use it for new work.
>
> **For all task and issue tracking, use beads (`bd`).**
>
> Existing specs under `specs/` are **reference only**. They describe historical intent but have been found to be **inconsistent** — the code and `bd` issues are authoritative.

### Rules

- Do **NOT** invoke `/bobkit.plan`, `/bobkit.tasks`, `/bobkit.implement`, or any other BobKit command for new features.
- Do **NOT** create new files under `specs/` or `.specify/`.
- Do **NOT** treat `specs/*/tasks.md` or `specs/*/plan.md` as ground truth.
- **DO** use `bd` for all planning, task breakdown, and progress tracking.
- Run `bd prime` at session start for current workflow context.

## Build & Test

```bash
cargo build --workspace          # build everything
cargo test --workspace           # run all tests
cargo test -p rotel-api          # test specific crate
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Architecture Overview

Rotel is an OpenTelemetry receiver and dashboard for local LLM users.

- `crates/rotel-storage` — embedded SQLite storage backend
- `crates/rotel-api` — axum HTTP API (logs, traces, metrics, health)
- `src/` — main binary, CLI, OTLP receiver

## Conventions & Patterns

- Rust 1.77+ stable
- Async via tokio; axum for HTTP
- `thiserror` for error types; no silent `?` swallowing
- Commit trailers: `Assisted-by: Claude Code`
