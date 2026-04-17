# Contributing to Rotel

Thank you for your interest in contributing to Rotel! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Pull Request Process](#pull-request-process)
- [Testing Requirements](#testing-requirements)
- [Code Style Guidelines](#code-style-guidelines)
- [Documentation](#documentation)
- [Reporting Issues](#reporting-issues)
- [Security Vulnerabilities](#security-vulnerabilities)

## Code of Conduct

We are committed to providing a welcoming and inclusive environment for all contributors. Please be respectful and professional in all interactions.

### Our Standards

- **Be respectful**: Treat everyone with respect and consideration
- **Be collaborative**: Work together to achieve common goals
- **Be constructive**: Provide helpful feedback and suggestions
- **Be inclusive**: Welcome diverse perspectives and backgrounds

## Getting Started

### Prerequisites

- **Rust**: 1.77+ (stable channel)
- **Git**: For version control
- **Pre-commit**: For automated code quality checks (optional but recommended)

### Setup Development Environment

1. **Fork and clone the repository**:
   ```bash
   git clone https://github.com/YOUR_USERNAME/rotel.git
   cd rotel
   ```

2. **Run the setup script**:
   ```bash
   ./scripts/setup-dev.sh
   ```

   This script will:
   - Verify Rust installation
   - Install required tools (cargo-nextest, cargo-llvm-cov, gitleaks)
   - Configure pre-commit hooks
   - Verify workspace compiles

3. **Verify setup**:
   ```bash
   cargo test
   cargo clippy --all-targets --all-features -- -D warnings
   cargo fmt --check
   ```

## Development Workflow

### 1. Create a Feature Branch

```bash
git checkout -b feature/your-feature-name
```

Use descriptive branch names:
- `feature/add-postgresql-backend`
- `fix/memory-leak-in-receiver`
- `docs/improve-quickstart-guide`

### 2. Make Your Changes

- Write clear, focused commits
- Follow the [code style guidelines](#code-style-guidelines)
- Add tests for new functionality
- Update documentation as needed

### 3. Run Quality Checks

Before committing, ensure all checks pass:

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test

# Check coverage (must be ≥80%)
./scripts/check-coverage.sh

# Run pre-commit hooks
pre-commit run --all-files
```

### 4. Commit Your Changes

Write clear, descriptive commit messages following [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

**Examples**:
```bash
git commit -m "feat(receiver): add support for OTLP/HTTP compression"
git commit -m "fix(storage): resolve memory leak in query cache"
git commit -m "docs(readme): update installation instructions"
```

### 5. Push and Create Pull Request

```bash
git push origin feature/your-feature-name
```

Then create a pull request on GitHub.

## Pull Request Process

### Before Submitting

Ensure your PR:
- [ ] Passes all CI checks (tests, linting, security)
- [ ] Includes tests for new functionality
- [ ] Updates documentation if needed
- [ ] Has a clear, descriptive title
- [ ] References related issues (e.g., "Fixes #123")
- [ ] Includes a summary of changes in the description

### PR Template

When creating a PR, use this template:

```markdown
## Summary
Brief description of changes (2-3 sentences)

## Changes
- List of key changes
- Organized by category if multiple types

## Motivation
Why these changes were needed

## Testing
- How changes were tested
- Test coverage information

## Breaking Changes
- List any breaking changes
- Include migration guide if applicable

## Related Issues
Fixes #123
Refs #456

## Checklist
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Breaking changes documented
- [ ] Changelog updated (if applicable)
```

### Review Process

1. **Automated Checks**: CI pipeline runs tests, linting, and security scans
2. **Code Review**: Maintainers review code for quality, correctness, and style
3. **Feedback**: Address any feedback or requested changes
4. **Approval**: Once approved, maintainers will merge your PR

### After Merge

- Your changes will be included in the next release
- You'll be credited in the release notes
- Thank you for contributing! 🎉

## Testing Requirements

### Test Coverage

- **Minimum coverage**: 80% for all code
- **Strict mode**: All tests must pass (no retries)
- **Performance**: Unit tests must complete in <30 seconds

### Writing Tests

**Unit Tests**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_works() {
        let result = my_function();
        assert_eq!(result, expected_value);
    }
}
```

**Integration Tests** (in `tests/integration/`):
```rust
#[test]
fn test_otlp_receiver_integration() {
    // Test OTLP receiver with real gRPC client
}
```

**E2E Tests** (in `tests/e2e/`):
```rust
#[test]
fn test_full_pipeline() {
    // Test complete data flow: ingest → store → query
}
```

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test integration_tests

# Run with coverage
cargo llvm-cov --all-features --workspace --html

# Run tests in parallel (faster)
cargo nextest run
```

## Code Style Guidelines

### Rust Style

Follow the [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/) and project-specific rules:

**Formatting** (enforced by `rustfmt`):
- Max line width: 100 characters
- Tab spaces: 4
- Edition: 2021

**Linting** (enforced by `clippy`):
- Cognitive complexity: ≤20
- Function lines: ≤80
- Function arguments: ≤5
- All clippy warnings treated as errors

**Naming Conventions**:
- Types: `PascalCase`
- Functions/variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Modules: `snake_case`

**Documentation**:
```rust
/// Brief description of function
///
/// # Arguments
///
/// * `param1` - Description of param1
/// * `param2` - Description of param2
///
/// # Returns
///
/// Description of return value
///
/// # Examples
///
/// ```
/// let result = my_function(arg1, arg2);
/// assert_eq!(result, expected);
/// ```
pub fn my_function(param1: Type1, param2: Type2) -> ReturnType {
    // Implementation
}
```

### Error Handling

- Use `Result<T, E>` for recoverable errors
- Use `panic!` only for unrecoverable errors
- Provide context with error messages
- Use `thiserror` or `anyhow` for error types

### Performance

- Avoid unnecessary allocations
- Use `&str` instead of `String` when possible
- Prefer iterators over loops
- Profile before optimizing

## Documentation

### Code Documentation

- All public APIs must have rustdoc comments
- Include examples in documentation
- Document panics, errors, and safety requirements

### User Documentation

Update relevant documentation in `docs/`:
- `quickstart.md` - Getting started guide
- `testing.md` - Testing documentation
- `troubleshooting.md` - Common issues and solutions

### Architecture Documentation

Update `ARCHITECTURE.md` for:
- New components or modules
- Architectural changes
- Design decisions

## Reporting Issues

### Bug Reports

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md):

- **Description**: Clear description of the bug
- **Steps to Reproduce**: Minimal steps to reproduce
- **Expected Behavior**: What should happen
- **Actual Behavior**: What actually happens
- **Environment**: OS, Rust version, Rotel version
- **Logs**: Relevant error messages or logs

### Feature Requests

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md):

- **Problem**: What problem does this solve?
- **Solution**: Proposed solution
- **Alternatives**: Alternative solutions considered
- **Additional Context**: Any other relevant information

## Security Vulnerabilities

**DO NOT** report security vulnerabilities in public issues.

Instead, email security@rotel.dev with:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

We will respond within 48 hours and work with you to address the issue.

## Questions?

- **GitHub Discussions**: For general questions and discussions
- **GitHub Issues**: For bug reports and feature requests
- **Documentation**: Check `docs/` for guides and references

## License

By contributing to Rotel, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).

---

Thank you for contributing to Rotel! Your efforts help make observability better for the LLM community. 🚀
