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

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

```bash
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file
rm -rf directory            # NOT: rm -r directory
```

Other commands: `apt-get -y`, `brew` with `HOMEBREW_NO_AUTO_UPDATE=1`, `scp`/`ssh` with `-o BatchMode=yes`.

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

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

1. **Run quality gates** (if code changed) — build, test, clippy, fmt
2. **Update beads** — close finished work, file new beads for remaining work
3. **Retrospective** — see below
4. **Push everything:**
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Hand off** — summarize what was done and what's next

**CRITICAL:** Work is NOT complete until `git push` succeeds. Never stop before pushing.

## Session Retrospective

Before ending each session, briefly consider:

1. **Process friction** — Did anything slow you down that could be avoided next time? (missing docs, unclear bead descriptions, flaky tests, confusing code patterns)
2. **Rules and standards** — Should any rule in this file or CLAUDE.md be added, clarified, or removed based on what happened this session?
3. **Documentation gaps** — Is ARCHITECTURE.md, README, or any other doc now stale because of changes made?
4. **Bead quality** — Were the bead descriptions clear enough to work from? If not, improve them for future agents.
5. **Tooling** — Would a reusable script, alias, or automation save time on recurring tasks?

If something actionable surfaces, either fix it immediately (if small) or create a bead for it. The goal is continuous improvement — each session should leave the project slightly easier to work on than before.

## Historical Note

This project was originally built with a "bobkit" spec-driven workflow. Those artifacts have been archived to `.archive/bobkit/` and are no longer used. Do not create or reference bobkit files.
