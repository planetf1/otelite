#!/usr/bin/env bash
# Rotate CHANGELOG.md [Unreleased] + changelog/*.md entries into [X.Y.Z] - DATE.
#
# Usage:
#   scripts/rotate-changelog.sh <new_version> [date]
#   scripts/rotate-changelog.sh --selftest
#
# Release notes come from two sources (either is fine, at least one is
# required):
#
#   1. Bullets already under '## [Unreleased]' in CHANGELOG.md.
#   2. Per-PR entry files in changelog/ (e.g. changelog/145.md). Each file
#      holds that PR's notes: bullet lines, optionally grouped under
#      '### Added' / '### Changed' / '### Fixed' / '### Removed' /
#      '### Internal' headings. A file with no heading defaults to Added.
#      Entry files are consumed (deleted) by the release commit.
#
# Behaviour:
#   1. Fails (exit 1) if neither source contains at least one bullet.
#   2. Merges both sources, grouping by section in canonical order
#      (Added, Changed, Fixed, Removed, Internal).
#   3. Replaces '## [Unreleased]' with a fresh empty section followed by
#      '## [<new_version>] - <date>' holding the merged sections.
#   4. Deletes the consumed changelog/*.md entry files.
#
# Date defaults to UTC YYYY-MM-DD if not provided.

set -euo pipefail

CHANGELOG="${CHANGELOG_PATH:-CHANGELOG.md}"
ENTRY_DIR="${ENTRY_DIR:-changelog}"

# ---------------------------------------------------------------------------
# Self-test: exercise the rotation in a temp dir, assert on the results.
# ---------------------------------------------------------------------------
if [[ "${1:-}" == "--selftest" ]]; then
    script=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "$0")
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    cd "$tmp"

    base_changelog() {
        cat > CHANGELOG.md <<'MD'
# Changelog

## [Unreleased]

## [0.0.1] - 2026-01-01

### Added

- First release.
MD
    }

    section() { # $1 = release version, $2 = release date, $3 = section name
        # Prints the bullet lines of '### <section>' inside '## [<v>] - <date>'.
        awk -v rel="## [$1] - $2" -v sec="### $3" '
            $0 == rel { in_rel = 1; next }
            in_rel && /^## / { in_rel = 0 }
            in_rel && $0 == sec { in_sec = 1; next }
            in_rel && in_sec && /^(##|###) / { in_sec = 0 }
            in_rel && in_sec { print }
        ' CHANGELOG.md
    }

    fail=0
    check() { # $1 = description, $2 = actual, $3 = expected
        if [[ "$2" == "$3" ]]; then
            echo "  ok: $1"
        else
            echo "  FAIL: $1"
            echo "    expected: $(printf '%q' "$3")"
            echo "    actual:   $(printf '%q' "$2")"
            fail=1
        fi
    }

    echo "case 1: entry files only (default Added, section merge, consumption)"
    base_changelog
    mkdir changelog
    echo "- Fix a bug in parsing." > changelog/2.md
    printf '### Fixed\n\n- Crash on empty input.\n\n### Internal\n\n- Bump a dependency.\n' > changelog/10.md
    "$script" 0.0.2 2026-01-02
    check "releases section exists" "$(grep -c '^## \[0.0.2\] - 2026-01-02' CHANGELOG.md)" "1"
    check "unreleased section exists" "$(grep -c '^## \[Unreleased\]' CHANGELOG.md)" "1"
    check "unreleased is empty" "$(awk '/^## \[Unreleased\]/{f=1;next} f&&/^## /{f=0} f' CHANGELOG.md | grep -c '^- ' || true)" "0"
    check "Added (no-heading file defaults to it)" "$(section 0.0.2 2026-01-02 Added | grep -c '^- ')" "1"
    check "Fixed kept" "$(section 0.0.2 2026-01-02 Fixed | grep -c '^- ')" "1"
    check "Internal kept" "$(section 0.0.2 2026-01-02 Internal | grep -c '^- ')" "1"
    check "entry files consumed" "$(ls changelog/*.md 2>/dev/null | wc -l | tr -d ' ')" "0"

    echo "case 2: [Unreleased] bullets only (backwards compatible)"
    mkdir -p changelog
    {
        echo "# Changelog"
        echo ""
        echo "## [Unreleased]"
        echo ""
        echo "### Changed"
        echo ""
        echo "- New default port."
        echo ""
        echo "## [0.0.1] - 2026-01-01"
        echo ""
        echo "### Added"
        echo ""
        echo "- First release."
    } > CHANGELOG.md
    "$script" 0.0.3 2026-01-03
    check "releases section exists" "$(grep -c '^## \[0.0.3\] - 2026-01-03' CHANGELOG.md)" "1"
    check "Changed kept" "$(section 0.0.3 2026-01-03 Changed | grep -c '^- ')" "1"

    echo "case 3: both sources merge (same section from both)"
    {
        echo "# Changelog"
        echo ""
        echo "## [Unreleased]"
        echo ""
        echo "### Fixed"
        echo ""
        echo "- Unreleased-side fix."
        echo ""
        echo "## [0.0.1] - 2026-01-01"
        echo ""
        echo "### Added"
        echo ""
        echo "- First release."
    } > CHANGELOG.md
    mkdir -p changelog
    printf '### Fixed\n\n- Entry-side fix.\n' > changelog/7.md
    "$script" 0.0.4 2026-01-04
    check "Fixed merged from both sources" "$(section 0.0.4 2026-01-04 Fixed | grep -c '^- ')" "2"

    echo "case 4: empty sources fail"
    base_changelog
    mkdir -p changelog
    if "$script" 0.0.5 2026-01-05 2>/dev/null; then
        check "empty release must fail" "succeeded" "failed"
    else
        check "empty release must fail" "failed" "failed"
    fi

    echo "case 5: unknown section heading fails"
    base_changelog
    mkdir -p changelog
    printf '### Bogus\n\n- No.\n' > changelog/9.md
    if "$script" 0.0.6 2026-01-06 2>/dev/null; then
        check "unknown heading must fail" "succeeded" "failed"
    else
        check "unknown heading must fail" "failed" "failed"
    fi

    if [[ "$fail" -ne 0 ]]; then
        echo "selftest: FAILED" >&2
        exit 1
    fi
    echo "selftest: all cases passed"
    exit 0
fi

# ---------------------------------------------------------------------------
# Real rotation.
# ---------------------------------------------------------------------------
if [[ $# -lt 1 ]]; then
    echo "usage: $0 <new_version> [date]" >&2
    exit 2
fi

NEW_VERSION="$1"
RELEASE_DATE="${2:-$(date -u +%Y-%m-%d)}"

if [[ ! -f "$CHANGELOG" ]]; then
    echo "error: $CHANGELOG not found" >&2
    exit 1
fi

# Extract content between '## [Unreleased]' and the next '## ' header.
unreleased_body=$(awk '
    /^## \[Unreleased\]/ { in_section = 1; next }
    in_section && /^## / { in_section = 0 }
    in_section { print }
' "$CHANGELOG")

# Collect entry files (dotfiles like .gitkeep/.template.md are ignored).
entry_files=()
if [[ -d "$ENTRY_DIR" ]]; then
    while IFS= read -r f; do
        entry_files+=("$f")
    done < <(find "$ENTRY_DIR" -maxdepth 1 -name '*.md' ! -name '.*' | sort)
fi

unreleased_has_bullets=1
grep -qE '^- ' <<<"$unreleased_body" || unreleased_has_bullets=0
entry_bullet_count=0
for f in "${entry_files[@]:-}"; do
    [[ -n "$f" && -f "$f" ]] || continue
    if grep -qE '^- ' "$f"; then
        entry_bullet_count=$((entry_bullet_count + 1))
    fi
done

if [[ "$unreleased_has_bullets" -eq 0 && "$entry_bullet_count" -eq 0 ]]; then
    cat >&2 <<EOF
error: no release notes found.

Releases must ship with user-facing notes. Add at least one bullet —
either under '## [Unreleased]' in $CHANGELOG, or (preferred for PRs) in
a new entry file '$ENTRY_DIR/<pr-number>.md' in this PR. See
CONTRIBUTING.md → Changelog.

If this is a pure plumbing release with no user impact, use:

    ### Internal
    - <one-line reason>
EOF
    exit 1
fi

# Build one stream of all note bodies. Entry files without any '### '
# heading default to Added. Unknown headings fail loudly.
stream=$(
    printf '%s\n' "$unreleased_body"
    for f in "${entry_files[@]:-}"; do
        [[ -n "$f" && -f "$f" ]] || continue
        if ! grep -qE '^### ' "$f"; then
            printf '### Added\n\n'
        fi
        cat "$f"
        printf '\n'
    done
)

merged=$(
    printf '%s\n' "$stream" | awk '
        BEGIN {
            n = split("Added Changed Fixed Removed Internal", order, " ")
            for (i = 1; i <= n; i++) known[order[i]] = 1
            cur = ""
        }
        /^### / {
            name = $2
            if (!(name in known)) {
                printf "error: unknown section heading [%s] (expected one of: Added, Changed, Fixed, Removed, Internal)\n", name > "/dev/stderr"
                bad = 1
                exit 1
            }
            cur = name
            next
        }
        {
            if (cur == "") cur = "Added"   # bullets before any heading default to Added
            body[cur] = body[cur] $0 "\n"
        }
        END {
            if (bad) exit 1
            for (i = 1; i <= n; i++) {
                name = order[i]
                b = body[name]
                # Trim leading and trailing blank lines of the section body.
                sub(/^[ \t\n]*\n?/, "", b)
                sub(/\n[ \t\n]*$/, "", b)
                if (b != "") {
                    printf "### %s\n\n%s\n\n", name, b
                }
            }
        }
    '
)

# Rewrite the changelog: fresh empty [Unreleased] + release section with
# the merged notes, replacing the old [Unreleased] section (heading and
# body — the body's content is now in $merged).
tmp=$(mktemp)
MERGED="$merged" awk '
    /^## \[Unreleased\]/ && !done {
        print "## [Unreleased]"
        print ""
        print "## [" ver "] - " date
        print ""
        # Command substitution stripped the trailing newlines of MERGED, so
        # add the blank line before the next section header explicitly.
        printf "%s\n\n", ENVIRON["MERGED"]
        done = 1
        skip = 1
        next
    }
    skip && /^## / { skip = 0 }
    skip { next }
    { print }
' ver="$NEW_VERSION" date="$RELEASE_DATE" "$CHANGELOG" > "$tmp"
mv "$tmp" "$CHANGELOG"

# Consume the entry files.
for f in "${entry_files[@]:-}"; do
    [[ -n "$f" && -f "$f" ]] || continue
    rm -f "$f"
done

echo "rotated CHANGELOG.md: [Unreleased] + ${#entry_files[@]} changelog entr(y/ies) → [$NEW_VERSION] - $RELEASE_DATE"
