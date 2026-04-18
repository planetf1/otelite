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

## Session End

Before ending a session:

1. Ensure all work is committed and pushed (`git push` must succeed)
2. Close completed beads, file new beads for unfinished work
3. Push beads: `bd dolt push`
4. Brief retrospective: could any rule, doc, or bead description be improved? If so, fix it or file a bead.

## Historical Note

This project was originally built with a "bobkit" spec-driven workflow. Those artifacts have been archived to `.archive/bobkit/` and are no longer used. Do not create or reference bobkit files.
