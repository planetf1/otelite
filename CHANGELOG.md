# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.10] - 2026-04-30

### Added

- **Web UI: JSON field detection and rendering** — attribute values and log bodies that contain
  JSON objects or arrays are now detected and rendered with syntax highlighting, collapse/expand
  toggle (▶/▼), and a `[raw]` button to toggle between formatted and original string. Scalar
  values (numbers, booleans, bare strings) are not affected.
- **Web UI: smart expand heuristic** — small JSON values (≤ 400 chars formatted) default to
  expanded; large values (LLM prompts, responses) default to collapsed.
- **Web UI: truncated JSON handling** — attribute values that appear to be truncated JSON
  (start with `{` or `[` but fail to parse) are repaired by closing open brackets, then
  pretty-printed and syntax-highlighted with a small amber `[truncated]` badge. Raw toggle
  still shows the original unmodified string.
- **Web UI: log ↔ trace cross-navigation** — `trace_id` in log details is a clickable link
  that switches to the Traces view pre-filtered to that trace. `trace_id` in trace detail
  links back to the Logs view filtered by that trace.
- **API: `trace_id` filter on `GET /api/logs`** — query parameter `?trace_id=` filters logs
  to a specific trace. Empty string is treated as no filter.

### Changed

- `GET /api/logs` response now includes `trace_id` and `span_id` fields in each log entry
  (these were already stored; now surfaced through the API query filter).

## [0.1.0-alpha] - 2026-04-17

### Added

- Initial alpha release
- Project constitution with 7 core principles
- Basic project structure and workspace configuration
- Development environment setup scripts
- Testing infrastructure foundation
  - cargo-nextest for fast test execution
  - cargo-llvm-cov for code coverage
  - Test fixtures and utilities
  - Automated test scripts
- Code quality infrastructure
  - clippy.toml with strict linting rules
  - rustfmt.toml with stable-compatible formatting
  - Pre-commit hooks for Rust
- Security infrastructure
  - gitleaks configuration for secret detection
  - Security scanning workflow
  - Pre-commit secret detection
- Documentation
  - Project README
  - Contributing guidelines
  - Architecture documentation
  - Changelog
