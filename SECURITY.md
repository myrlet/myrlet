# Security Policy

## Supported versions

Security support follows the release lines published in GitHub Releases.
When reporting an issue against a source checkout or development build, identify
the affected commit, deployment mode, and any locally modified components.

## Reporting a vulnerability

**Please do not report security vulnerabilities through public Issues, Discussions, or pull requests.**

Use GitHub's private vulnerability reporting for this repository:

<https://github.com/myrlet/myrlet/security/advisories/new>

If private vulnerability reporting is unavailable, do not publish exploit details or sensitive reproduction material in a public issue. Contact a repository owner through the Myrlet GitHub organization page and request a private reporting channel:

<https://github.com/myrlet>

When reporting, include as much of the following as you can safely provide:

- Affected commit, tag, or release and the deployment topology.
- Impact and security boundary involved (Hub API, Worker connection, Scope/Owner authorization, credential handling, execution backend, or container image).
- Reproduction steps or a proof of concept, with secrets and personal data removed.
- Any known mitigations and whether the issue is known to be actively exploited.

The maintainers will acknowledge and assess reports, coordinate remediation, and agree on an appropriate disclosure timeline with the reporter. Please allow time for investigation; no response-time or remediation SLA is currently promised.

## Scope and security boundaries

The architecture and trust assumptions are documented in [Security Architecture](docs/architecture/security.md). Its credential matrix and [read visibility](docs/architecture/security.md#read-visibility) describe the current binary. Operators provide isolation for Workers and execution backends. Report a vulnerability if an implementation violates a documented boundary or exposes credentials, data, or authority beyond the intended Scope.

Please distinguish a security vulnerability from a general bug or hardening suggestion. For non-sensitive bugs, use the public [Bug Report issue form](https://github.com/myrlet/myrlet/issues/new/choose).
