# Security Policy

## Supported Versions

Security updates are provided for the latest version on the `main` branch unless otherwise stated.

## Reporting a Vulnerability

Please do not report security vulnerabilities through public GitHub issues.

**Preferred:** open a private report at
<https://github.com/anderstvoss/vertix-reconstruction/security/advisories/new>
(GitHub Private Vulnerability Reporting).

There is no email contact; use the private report above.

Include:

- Affected version or commit
- Reproduction steps
- Impact
- Suggested fix, if known

Initial response: within 7 days. Coordinated disclosure preferred; we
will work with you on a timeline before any public advisory.

## Defensive Posture

This repo enforces:

- Pre-commit secret scanning (gitleaks) plus a suite of custom blockers for
  env files, private keys, credentials, local paths, private IPs, cloud
  storage URIs, and binary artifacts.
- Pre-push: gitleaks full-tree scan + tracked-file blocker + local-paths
  guard + `cargo deny check` + `cargo audit`.
- `unsafe` is forbidden; if a crate ever needs FFI, every `unsafe` block
  must carry a `// SAFETY:` justification (`clippy::undocumented_unsafe_blocks`).
- Python helpers, if any, are linted for defects and security footguns
  (ruff pyflakes + bandit rules) and their unit tests run in CI.
- CodeQL (`security-extended`) for Rust and for the workflow files.
- CI on every PR and push to `main`: `cargo fmt`, `clippy -D warnings`,
  `cargo check`, `cargo test` across Ubuntu + macOS + Windows; the full
  pre-commit + pre-push policy replay on the same matrix; `cargo-deny`
  + `cargo-audit`; `actions/dependency-review-action` on PRs.
- All third-party actions SHA-pinned; `step-security/harden-runner`
  with egress-policy `block` and an explicit allowlist on every Linux
  job.
- Weekly full-history gitleaks scan over every ref (branches and
  tags), plus `scripts/deep-scan.sh` for ad-hoc operator runs.
- OpenSSF Scorecard on push to `main`, weekly cron, and
  branch-protection-rule events; SARIF published to the Security tab.
- Dependabot alerts + automated security updates.
- Codeowner review required on `.github/`, hooks, security docs, and
  dependency manifests.
- A full-history secret scan is a precondition for changing repository
  visibility.

For the end-to-end setup procedure (reusable across projects), see
[`docs/REPO-SETUP.md`](docs/REPO-SETUP.md).
