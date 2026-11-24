# Security workflow fix — url RUSTSEC-2026-0592 + chrono/chrono-tz unmaintained advisories

Date: 2026-11-24
Branch: `security/fix-audit-2026-12` (local only; not pushed, no PR, nothing posted to GitHub)

## 1. Vulnerability fix: url 2.5.8 → 2.5.9 (RUSTSEC-2026-0592)

`RUSTSEC-2026-0592`: stack overflow in `Url::origin()` on nested `blob:` URLs
(DoS, all versions <= 2.5.8, patched >= 2.5.9). `url` is a transitive
dependency here (not declared in any workspace `Cargo.toml`), so the fix is a
lockfile bump, matching the same bump merged in planetf1/oxllm PR #18.

Command run:

```bash
cargo update -p url --precise 2.5.9
```

Resulting `Cargo.lock` changes:

- `url` 2.5.8 → 2.5.9
- `form_urlencoded` 1.2.2 → 1.2.3 (pulled along by the same update; it is a
  `url` dependency, same servo/rust-url crate family)

After the bump, `cargo audit` reports no vulnerabilities for `url`; only the
two unmaintained warnings in §2 remain.

## 2. Denied warnings: chrono and chrono-tz

Correction to the task premise: there are **two distinct advisories**, not one.
The Security workflow (`cargo audit --deny warnings`, `.github/workflows/security.yml:51`)
fails on both:

- `RUSTSEC-2026-0543` — **chrono** is unmaintained (v0.4.45)
- `RUSTSEC-2026-0555` — **chrono-tz** is unmaintained (v0.10.4)

Both are `informational = "unmaintained"` advisories dated 2026-09-14, both
pointing at https://github.com/chronotope/chrono/issues/1768, with
`patched = []` (no version of either crate is a fix). Neither is a
vulnerability. The chrono advisory recommends `jiff` or `time` as
alternatives; the chrono-tz advisory names no replacement.

### Who pulls them in

Direct workspace dependencies:

- `chrono` (workspace dep, `Cargo.toml:39`): `otelite-core`, `otelite-api`,
  `otelite-storage`, `otelite-tui`, `otelite`
- `chrono-tz` (workspace dep, `Cargo.toml:40`): `otelite-api`,
  `otelite-storage`, `otelite-tui`, `otelite`
- `rusqlite` 0.41 with the `chrono` feature
  (`crates/otelite-storage/Cargo.toml:18`) also depends on chrono, so even
  removing the direct deps would not remove it from the tree.

Usage scale: 22 source files across 5 crates, ~105 chrono/tz type or call
sites; `chrono_tz::Tz` specifically in 6 files (`otelite-api/src/api/genai.rs`,
`otelite-storage/src/sqlite/reader.rs`, `otelite-tui/src/app.rs`,
`otelite-tui/src/ui/usage.rs`, `otelite/src/commands/model_performance.rs`,
`otelite/src/commands/usage.rs`).

### Options

1. **Replace** with `jiff` (or `time` + a tz crate): correct long-term answer,
   but a large cross-cutting refactor. It touches every timestamp path in 5
   crates, the `rusqlite` chrono feature (SQLite value conversion types), the
   API surface (`otelite-api` exposes chrono types via serde), and the TUI
   rendering. Well beyond the scope of unbreaking the Security workflow, and
   it would ride a large behavioural-change risk on a security-fix PR.
2. **Drop** the dependency: not viable. Timestamps and timezone-aware display
   are core to otelite's data model and UI; nothing here is incidental usage
   that can simply be deleted.
3. **Ignore both advisories** in `.cargo/audit.toml`, with comments, and track
   the migration separately.

### Recommendation: option 3

Ignore both advisories in `.cargo/audit.toml` now, and file a tracking issue
for the `jiff` migration. Reasoning:

- The advisories are informational (unmaintained), not vulnerabilities. There
  is no patched version to move to; `--deny warnings` can never go green on
  this tree any other way.
- This is the established precedent in this repo's audit config: two other
  unmaintained-with-no-fix advisories are already ignored with comments
  (`RUSTSEC-2024-0436` paste, `RUSTSEC-2024-0370` proc-macro-error). The
  difference here is that these are direct rather than transitive deps, which
  is exactly why the replacement deserves its own tracked issue instead of
  being squeezed into this fix.
- Replacement (option 1) is the right follow-up but a separate, deliberate
  piece of work; option 2 is not real.

Proposed diff, **applied after approval on 2026-11-24** (the option 3 recommendation above was approved):

```diff
--- a/.cargo/audit.toml
+++ b/.cargo/audit.toml
@@ -9,3 +9,11 @@ ignore = [
     # Only triggered by mutable iteration of the LRU cache, which ratatui does not do.
     "RUSTSEC-2026-0002",
+    # chrono - unmaintained (informational, no vulnerability, no patched version);
+    # direct dep in 5 crates + via rusqlite's chrono feature. Track migration to jiff.
+    "RUSTSEC-2026-0543",
+    # chrono-tz - unmaintained (informational, no vulnerability, no patched version);
+    # direct dep in 4 crates. Track migration alongside chrono.
+    "RUSTSEC-2026-0555",
 ]
```

## 3. Verification — exact commands and real results

All run on branch `security/fix-audit-2026-12` with only the `Cargo.lock`
change applied (the chrono-tz decision in §2 was left as a recommendation, so
nothing else was modified).

```bash
cargo update -p url --precise 2.5.9
cargo audit
cargo audit --deny warnings        # what CI runs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
node --test "crates/otelite-api/tests/js/*.test.mjs"
node scripts/check_analytics_nav.mjs
```

Results:

- `cargo update -p url --precise 2.5.9` — ok; url 2.5.8 → 2.5.9,
  form_urlencoded 1.2.2 → 1.2.3
- `cargo audit` — **no vulnerabilities**; 2 warnings (chrono, chrono-tz
  unmaintained), both informational
- `cargo audit --deny warnings` — **fails, exit 1**: "2 denied warnings
  found" (the chrono/chrono-tz advisories of §2; RUSTSEC-2026-0592 is gone).
  CI stays red until the §2 decision is applied
- `cargo build --workspace` — ok (finished in 19.08 s)
- `cargo test --workspace` — ok, exit 0; 1704 tests passed, 73 test binaries
  "test result: ok", 0 failed
- `cargo clippy --workspace --all-targets -- -D warnings` — **FAILED, exit
  101**, but **pre-existing on main and unrelated to this change**: duplicated
  `#[allow(clippy::too_many_arguments)]` attribute at
  `crates/otelite-core/src/storage.rs:264-265` and `357-358` (local clippy
  0.1.100 flags `clippy::duplicated_attributes`). Evidence it is pre-existing:
  `git status`/`git diff` show the only modification on this branch is
  `Cargo.lock`; the source file is byte-identical to main. Trivial fix if
  wanted: delete the duplicate line at 265 and 358 (keep the commented one)
- `cargo fmt --check` — ok, exit 0
- `node --test crates/otelite-api/tests/js/*.test.mjs` — ok, 77/77 pass
- `node scripts/check_analytics_nav.mjs` — ok ("analytics navigation
  integrity: OK — 33 reports, 5 groups")

## 4. Follow-ups (not done here)

1. File a tracking issue for the chrono/chrono-tz → jiff migration.
2. Fix the pre-existing clippy `duplicated_attributes` in
   `crates/otelite-core/src/storage.rs` — **done**, see §5.

### Release cycle — not run, and why

The release cycle was **not** run in this session. Only a local commit exists;
nothing is pushed, so nothing release-shaped can happen:

- Entry file: `changelog/<PR-number>.md` needs a PR number, and no PR exists
  (opening one was out of scope for this session). `scripts/rotate-changelog.sh`
  ignores only dotfiles (`.gitkeep`, `.template.md`) and consumes every other
  `changelog/*.md`, so the filename must not be invented — it would be merged
  and deleted under a wrong number.
- Everything after that is automated, not a manual step: once the fix PR
  merges to main, `bump-and-tag.yml` rotates `[Unreleased]` plus every
  `changelog/*.md` into a new version section, opens the auto-merged
  `chore: release vX.Y.Z` PR, then tags and publishes on merge. Current version
  is 0.1.160, so the next would be 0.1.161.
- `CHANGELOG.md` `## [Unreleased]` is empty and was deliberately left alone
  (per-PR entry files are the workflow; the release PR does the rotation).

Ready-to-use entry text, to be written as `changelog/<PR-number>.md` when the
PR is opened:

```markdown
### Fixed

- A crafted `blob:` URL with deeply nested prefixes can no longer crash the
  server or CLI: `url` is updated to 2.5.9, which fixes a stack overflow in
  `Url::origin()` (RUSTSEC-2026-0592).
```

If the fix PR is judged not user-visible, use `### Internal` with an equivalent
plumbing bullet instead; the release gate only requires at least one bullet.

## 5. Applied after approval (2026-11-24)

- `.cargo/audit.toml`: added ignores for RUSTSEC-2026-0543 (chrono) and
  RUSTSEC-2026-0555 (chrono-tz) per §2.
- `crates/otelite-core/src/storage.rs`: removed the duplicated
  `#[allow(clippy::too_many_arguments)]` at lines 264 and 357 (kept the
  commented occurrence at 265/358), fixing the pre-existing
  `clippy::duplicated_attributes` failure.

Re-run after both changes:

```bash
cargo audit --deny warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo fmt --check
```

- `cargo audit --deny warnings` — ok, exit 0, "0 denied warnings found"
  (url vulnerability fixed in §1, unmaintained advisories now ignored with
  comments)
- `cargo clippy ... -D warnings` — ok, exit 0
- `cargo test --workspace` — ok, exit 0, 1704 tests passed, 0 failed (same
  count as before the changes: no tests added or removed by either change)
- `cargo fmt --check` — ok, exit 0

Both changes committed locally on `security/fix-audit-2026-12`. Not pushed;
no PR opened; nothing posted to GitHub.
