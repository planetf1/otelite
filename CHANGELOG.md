# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project structure with Cargo workspace
- Development quality infrastructure (Feature 007)
  - Comprehensive testing framework with unit, integration, and e2e tests
  - Code quality tools: rustfmt, clippy with strict linting rules
  - Secret detection with gitleaks
  - Pre-commit hooks for automated quality checks
  - CI/CD pipeline with GitHub Actions
  - Security scanning workflow
- Project documentation
  - README.md with quick start guide
  - CONTRIBUTING.md with development guidelines
  - ARCHITECTURE.md with system design documentation
  - CHANGELOG.md for tracking changes

### Changed
- N/A

### Deprecated
- N/A

### Removed
- N/A

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
