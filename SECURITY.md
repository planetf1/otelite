# Security Policy

## Reporting a Vulnerability

**Do not report security vulnerabilities in public GitHub Issues.**

Preferred: use GitHub's private vulnerability reporting:

1. Go to the [Security Advisories page](https://github.com/planetf1/otelite/security/advisories/new)
2. Click **Report a vulnerability**
3. Fill in the description, steps to reproduce, and potential impact

Alternative: email **[jonesn@uk.ibm.com](mailto:jonesn@uk.ibm.com)** with the same details.

Either way, please include:

- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- Any suggested mitigations (optional)

You will receive an acknowledgement within 48 hours. Credit will be given in the release notes unless you prefer to remain anonymous.

## Supported Versions

This project is in early development. Only the latest released version receives security fixes.

## Scope

Otelite is a local development tool that binds only to `localhost` by default. It is not designed for production or internet-facing deployments. Reports about exposing it to untrusted networks are out of scope.

## Supply Chain Security

- **Dependency auditing:** `cargo audit` runs on every CI push and PR, denying advisories for known vulnerabilities and unmaintained crates.
- **SAST:** Semgrep scans all code on every push and PR.
- **Signed releases:** Crates are published only from tagged commits via a dedicated GitHub Actions workflow using a scoped `CARGO_REGISTRY_TOKEN` secret.
- **Minimal permissions:** CI jobs run with least-privilege GitHub token permissions. The release workflow is the only job with `contents: write`.
