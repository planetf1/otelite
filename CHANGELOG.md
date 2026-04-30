# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.9] - 2026-04-30

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

### Fixed
- N/A

### Security
- Implemented secret detection with gitleaks to prevent credential leaks
- Added security scanning workflow with cargo-audit, cargo-deny, and semgrep

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

### Known Issues
- OTLP receiver not yet implemented (planned for 0.2.0)
- Storage backend not yet implemented (planned for 0.2.0)
- Dashboard UI not yet implemented (planned for 0.3.0)
- Query engine not yet implemented (planned for 0.3.0)

### Notes
- This is an alpha release focused on establishing development infrastructure
- The project is not yet functional for end users
- Breaking changes are expected in future releases

---

## Release Notes Format

Each release should include:

### Added
New features and capabilities

### Changed
Changes to existing functionality

### Deprecated
Features that will be removed in future releases

### Removed
Features that have been removed

### Fixed
Bug fixes

### Security
Security-related changes and fixes

---

## Version History

- **0.1.0-alpha** (2026-04-17): Initial alpha release with development infrastructure
- **Unreleased**: Current development version

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on contributing to Otelite.

When making changes, please update this changelog following the [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) format.

---

**Maintained by**: Otelite Contributors  
**License**: Apache 2.0
