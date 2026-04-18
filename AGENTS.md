# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd dolt push          # Push beads data to remote
```

## Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files
- No agent attribution in commits — do not add Co-Authored-By, Assisted-by, or similar trailers

## How to Start a Session

1. Run `bd ready` to see available beads sorted by priority
2. Pick the highest-priority unblocked bead
3. Run `bd show <id>` to read the full description — it has step-by-step instructions
4. Run `bd update <id> --claim` to claim it
5. Follow the instructions in the bead description precisely
6. When done: quality gates, commit, push, close bead (see below)
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
3. Commit with a clear message describing what was done (no agent attribution trailers)
4. Push immediately: `git push`
5. Close the bead: `bd close <id> --reason "what was done"`
6. Push beads data: `bd dolt push`

**Do NOT batch multiple beads into one commit.** Each bead = one commit + push.

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** to avoid hanging on confirmation prompts.

```bash
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file
rm -rf directory            # NOT: rm -r directory
```

## Quality Gates

ALL code changes must pass before committing:

```bash
cargo build --workspace                                    # must compile
cargo test --workspace                                     # all tests pass
cargo clippy --workspace --all-targets -- -D warnings      # zero warnings
cargo fmt --check                                          # formatting ok
```

### Code Standards

- No `unwrap()` or `expect()` on user-facing code paths (tests are fine)
- No silent error swallowing — propagate errors with context
- No TODO comments without a corresponding bead (`bd create`)
- Error messages must explain: what was attempted, what failed, what to try next
- Tests must assert specific values, not just "doesn't panic"
- No `#[allow(dead_code)]` without a comment explaining why the code is needed

## Working with Beads

Each bead has a detailed description with step-by-step instructions, exact file paths, verification commands, and acceptance criteria.

**Always read the full bead description** (`bd show <id>`) before starting work. Follow the instructions precisely.

## Picking What to Work On

Use beads priorities, labels, and dependencies to decide:

```bash
bd ready                    # Show all unblocked beads, sorted by priority
bd ready -l pipeline        # Show only pipeline-related beads
bd ready -l cli             # Show only CLI beads
bd ready -l bugfix          # Show only bug fixes
```

### Priority rules

- **P0 (critical):** Must be done first. These form a dependency chain — do them in order. They connect the core pipeline (OTLP → storage → API).
- **P1 (high):** Core functionality and design. Do after P0 chain completes (some are unblocked earlier). The `cli` label P1 bead for noun-verb design should be done early as it shapes later CLI work.
- **P2 (medium):** Quality, polish, and secondary features. Bug fixes (`bugfix` label) first, then `quality` label, then features.
- **P3 (low):** Advanced features, documentation, nice-to-haves. Do these last.

### Label meanings

| Label | Meaning |
|-------|---------|
| `pipeline` | Core OTLP → storage → API data flow |
| `cli` | CLI commands, output, structure |
| `tui` | Terminal UI |
| `web` | Web dashboard |
| `api` | REST API endpoints |
| `genai` / `llm` | LLM/GenAI-specific features |
| `quality` | Tests, clippy, error handling, code review |
| `docs` | Documentation |
| `infra` | Config, logging, CI, service mode |
| `ai` | AI chat integration, MCP |
| `cleanup` | Remove dead code, archive artifacts |
| `bugfix` | Bug fixes |
| `testing` | Test suites |
| `search` | Search and query features |

### Dependency rules

If `bd show <id>` lists dependencies, those must be completed first. The tool enforces this — blocked beads won't appear in `bd ready` output.

### Within the same priority, prefer

1. Bug fixes (`bugfix` label)
2. Beads that unblock other beads (check with `bd dep list <id> --direction up`)
3. Smaller beads (quicker wins, faster feedback)

## When You're Stuck

If you can't complete a bead:

1. **Compilation error you can't fix:** Read the error carefully. Check if a dependency bead should have been done first (`bd dep list <id>`). If so, stop — the bead is blocked.
2. **Test failure you don't understand:** Run the single failing test with `cargo test -p <crate> -- <test_name> --nocapture` to see output.
3. **Unclear bead description:** Don't guess. File a new bead describing what's unclear, add it as a blocker, and move to the next unblocked bead.
4. **Bead is too large:** If a bead involves more than 3 files or 2 components, break it into smaller beads with `bd create` and add dependencies. Close the original as a parent.

**Never submit incomplete work.** If you can't finish a bead, leave it as `in_progress` with a comment (`bd update <id> --notes "got stuck on X"`) and move on.

## Definition of Done

A bead is complete when ALL of these are true:

1. Acceptance criteria in `bd show <id>` are met
2. All quality gates pass (build, test, clippy, fmt)
3. Changes are committed with a clear message
4. `git push` succeeded
5. `bd close <id> --reason "..."` called with a specific reason

## Session End

Before ending a session:

1. Ensure all work is committed and pushed (`git push` must succeed)
2. Close completed beads, file new beads for unfinished work
3. Push beads: `bd dolt push`
4. Brief retrospective: could any rule, doc, or bead description be improved? If so, fix it or file a bead.

## Historical Note

This project was originally built with a "bobkit" spec-driven workflow. Those artifacts have been archived to `.archive/bobkit/` and are no longer used. Do not create or reference bobkit files.
