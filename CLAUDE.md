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

## Quality Standards

These rules apply to ALL code changes:

### Before committing

1. `cargo build --workspace` — must compile cleanly
2. `cargo test --workspace` — all tests must pass
3. `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
4. `cargo fmt --check` — formatting must pass

### Code quality rules

- **No `#[allow(dead_code)]`** unless the code is genuinely needed soon (add a comment explaining why)
- **No `unwrap()` or `expect()`** on user-facing code paths (tests are fine)
- **No silent error swallowing** — every `?` should propagate to a meaningful error message
- **No TODO comments** without a corresponding bead — if work is deferred, create a bead for it
- **Error messages must include context** — what was attempted, what failed, what to try next
- **Tests must assert specific values** — not just "doesn't panic"

### Bead workflow

- Before starting work: `bd update <id> --claim` to claim the bead
- After completing: `bd close <id> --reason "what was done"` with a clear reason
- If blocked: create a new bead for the blocker and add a dependency
- Read the bead's full description with `bd show <id>` before starting — it has step-by-step instructions
