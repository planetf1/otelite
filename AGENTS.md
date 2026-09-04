# Agent Instructions

This file provides workflow guidance for AI agents working on Otelite. For technical context (architecture, crate structure, gotchas), see **CLAUDE.md**.

## Issue Tracking

Issues are tracked on GitHub: https://github.com/planetf1/otelite/issues

```bash
gh issue list --state open --label "priority:p2"   # Find available work
gh issue list --state open --assignee @me           # Your current work
gh issue view <number>                              # Read issue details
gh issue comment <number> --body "Starting this"   # Claim it
```

Labels: `priority:p1`–`p4`, `area:*` (receiver/storage/server/cli/tui/web/core/ci/ai), `type:*` (bug/feature/test/docs/refactor/chore).

## How to Start a Session

1. Run `gh issue list --state open --label "priority:p2" --limit 20` to see available work
2. Pick the highest-priority unblocked issue
3. Read the full issue: `gh issue view <number>`
4. Check for blocking dependencies noted in the issue body
5. Follow the instructions in the issue precisely

## Commit and Push After Every Issue

After completing each issue:

1. Run all quality gates (see below)
2. Commit with a clear message — no agent attribution trailers
3. Push: `git push`
4. Comment on the issue with what was done, then close it:
   ```bash
   gh issue comment <number> --body "Done: <what was done>"
   gh issue close <number>
   ```

**Do not batch multiple issues into one commit.** One issue = one commit + push.

## Non-Interactive Shell Commands

Always use non-interactive flags to avoid hanging:

```bash
cp -f source dest
mv -f source dest
rm -f file
rm -rf directory
```

## Quality Gates

All code changes must pass before committing:

```bash
cargo build --workspace                                    # must compile
cargo test --workspace                                     # all tests pass
cargo clippy --workspace --all-targets -- -D warnings      # zero warnings
cargo fmt --check                                          # formatting ok
```

### Local coverage (optional)

```bash
rustup run stable cargo llvm-cov --workspace --all-features --summary-only
```

**Why `rustup run stable`?** The repo pins `channel = "stable"` in `rust-toolchain.toml`. On macOS with Homebrew Rust, the system `cargo` may lack `llvm-tools`. `rustup run stable` uses the rustup-managed toolchain which has them.

## Code Standards

- No `unwrap()` or `expect()` on user-facing code paths (tests are fine)
- No silent error swallowing — every `?` propagates with context
- No TODO comments without a corresponding GitHub issue
- Error messages explain: what was attempted, what failed, what to try next
- Tests assert specific values, not just "doesn't panic"
- No `#[allow(dead_code)]` without a comment explaining why the code is needed

## Testing Requirements

**New code requires new tests. No exceptions.**

| What you added | Minimum test required |
|---|---|
| New HTTP endpoint | Integration test covering at least: empty state + data present |
| New function or method | Unit test in the same file's `#[cfg(test)]` module |
| Bug fix | Regression test that would have failed before the fix |
| New CLI command | Integration test using mockito or equivalent |
| Refactor | No net reduction in test count; all existing tests still pass |

Verify new tests were actually written before closing an issue:

```bash
cargo test --workspace 2>&1 | grep "test result: ok" | awk '{sum += $4} END {print sum " total tests"}'
```

If the count is the same as before and you added production code, you missed tests.

**Run tests after writing them.** Don't just add test functions — run `cargo test -p <crate>` and confirm they appear in the output and pass.

### Test isolation — test data and test ports only

- Tests never touch the developer's real otelite data (`~/.otelite/data`) or
  the running daemon. Every spawned `serve`/`start` process gets a `tempfile`
  data dir (`OTELITE_DATA_DIR` / `--storage-path`).
- Tests never bind or probe the standard ports (OTLP 4317/4318, or any fixed
  port). Every port a test process listens on is ephemeral (a `free_port()`
  helper), passed through `--addr` / `OTELITE_OTLP_GRPC_PORT` /
  `OTELITE_OTLP_HTTP_PORT` so both the spawned process and any discovery
  assert agree on it. Fixed-port tests race with the dev daemon and with
  parallel test binaries (observed CI failures 2026-09-04).
- The UI stays runnable for testing on a test port with test data:
  `otelite serve --addr 127.0.0.1:<free-port> --storage-path
  <tempdir>/otelite.db` (plus the OTLP env overrides). Anything that
  hardcodes a default port or data path in a way that prevents this is a
  bug, not a test limitation.

## Picking What to Work On

- **type:bug** before **type:feature** at the same priority
- Issues that unblock other issues first (check "Blocks: #N" in the issue body)
- Smaller issues over larger ones (quicker wins, faster feedback)

### Priority guide

| Priority | Meaning |
|----------|---------|
| `priority:p1` | High — blocking or urgent |
| `priority:p2` | Medium — should be done soon |
| `priority:p3` | Lower — nice to have |
| `priority:p4` | Backlog — deferred |

## When You're Stuck

1. **Compilation error you can't fix:** Check if a dependent issue should be done first. If so, stop — the issue is blocked.
2. **Test failure you don't understand:** Run with `cargo test -p <crate> -- <test_name> --nocapture`.
3. **Unclear issue description:** Don't guess. Leave a comment on the issue asking for clarification, then move to the next unblocked issue.
4. **Issue is too large:** Break it into smaller issues via `gh issue create`, add cross-references, close the original as a parent.

**Never submit incomplete work.** If you can't finish, leave a comment on the issue describing where you stopped.

## Definition of Done

An issue is complete when ALL of these are true:

1. Acceptance criteria in the issue are met
2. All quality gates pass (build, test, clippy, fmt)
3. New production code has new tests
4. **`CHANGELOG.md` `[Unreleased]` section is updated** if the change is user-visible
   (see "Changelog discipline" below)
5. Changes are committed with a clear message
6. `git push` succeeded
7. Issue is commented and closed

## Changelog discipline

`CHANGELOG.md` is the user-facing record of what otelite can do. Keep it current.

**When to add an entry**: any change a user can observe — new CLI flags, new API
endpoints, new TUI panels, web UI features, fixed bugs they were hitting, schema
or behaviour changes. Skip entries for: pure refactors, internal-only test
changes, CI/release plumbing, dependency bumps with no behaviour change.

**How to write entries**:

- Describe what the user can now *do* (or no longer has to work around), not the
  implementation. "Add `query_cache_hit_rate`" is bad; "See per-model cache hit
  rate to spot prompt-caching wins" is good.
- One bullet per user-visible change. Group related polish under one bullet.
- Use `### Added` / `### Changed` / `### Fixed` / `### Removed` / `### Internal`
  per [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
- Flag breaking changes prominently.

**Workflow (per-PR entry files)**:

1. While working on a PR, add `changelog/<PR-number>.md` (start from
   `changelog/.template.md`) with the entry bullets. Do not edit
   `CHANGELOG.md` — it is the release-generated record, and concurrent
   edits to it are what used to make open PRs clash on every release
   rotation.
2. At release time, the bump-and-tag CI workflow runs
   `scripts/rotate-changelog.sh`, which merges any `[Unreleased]` bullets
   and every `changelog/*.md` entry into `[X.Y.Z] - YYYY-MM-DD` (canonical
   section order: Added, Changed, Fixed, Removed, Internal) and deletes the
   consumed entry files. The script **fails the release** if neither source
   has at least one bullet.
3. Bullets may still be appended under `## [Unreleased]` directly for
   changes made on main itself; both sources merge into the release section.
4. For pure-plumbing pushes with no user impact, add an `### Internal`
   bullet (entry file or `[Unreleased]`). This satisfies the gate without
   inventing fake user-facing copy.
5. If the bump fails because no notes were found, add the missing entry and
   push — the workflow retries automatically on the next push to main.

Test the script: `scripts/rotate-changelog.sh --selftest`.
Dry-run the rotation locally:

```bash
scripts/rotate-changelog.sh 0.1.99 2026-05-15  # writes to CHANGELOG.md — git restore after
```

## Session End

1. `git status` — should show nothing uncommitted
2. If there are uncommitted changes, commit and push them
3. Close any completed issues
4. Leave comments on in-progress issues describing current state

## Historical Note

This project was originally developed with AI assistance using a spec-driven workflow. Do not create specification artifacts or template scaffolding files.
