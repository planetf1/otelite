#!/usr/bin/env bash
# Warn if beads (bd) is not installed — run by pre-commit as a non-blocking check
if ! command -v bd >/dev/null 2>&1; then
    echo "  Tip: install beads (bd) for issue tracking. See .beads/README.md"
fi
exit 0
