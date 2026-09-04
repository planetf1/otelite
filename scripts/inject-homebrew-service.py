#!/usr/bin/env python3
"""Inject a `service do` block into a cargo-dist Homebrew formula.

Inserts the block before the final `end` that closes the Formula class so
`brew services start otelite` works. Used by .github/workflows/release.yml.

An inline heredoc cannot live in the workflow's `run: |` block: the heredoc
body is unindented, which makes the whole file invalid YAML and breaks
GitHub's workflow definition (no triggers, dispatch 422).

Usage: python3 inject-homebrew-service.py <path-to-formula.rb>
"""

import re
import sys

SERVICE_BLOCK = """
  service do
    run [opt_bin/"otelite", "serve"]
    keep_alive true
    log_path var/"log/otelite.log"
    error_log_path var/"log/otelite.log"
  end
"""


def main() -> None:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <formula.rb>", file=sys.stderr)
        sys.exit(2)
    path = sys.argv[1]
    text = open(path).read()
    # Insert before the last bare `end` in the file (closes the Formula class)
    new_text, n = re.subn(r"\nend\s*$", SERVICE_BLOCK + "\nend\n", text, count=1)
    if n != 1:
        print(f"error: could not find final 'end' in {path}", file=sys.stderr)
        sys.exit(1)
    open(path, "w").write(new_text)


if __name__ == "__main__":
    main()
