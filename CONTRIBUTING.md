# Contributing to TEMPLATE_PROJECT_NAME

Thanks for your interest. Please read this file end-to-end before opening a PR.

## Development setup

Run once after cloning:

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks
```

`pre-commit` itself must be installed system-wide (via pipx or pip — see
<https://pre-commit.com/#install>). The committed wrappers in `.githooks/`
delegate to whichever `pre-commit` is on `PATH`.

If you previously ran `pre-commit install` on this clone, remove the
now-stale wrappers so `core.hooksPath` is the only source:

```bash
rm -f .git/hooks/pre-commit .git/hooks/pre-push
```

## Required gates

Every commit runs pre-commit hooks: gitleaks, custom blockers for env
files / keys / local paths / private IPs / cloud URIs, plus
`cargo fmt`, `cargo check`, `cargo clippy -D warnings`, `cargo test`.

Every push runs additional pre-push hooks: gitleaks full-tree scan,
tracked-file blocker, local-paths guard, `cargo deny check`, and
`cargo audit`.

CI re-runs the cargo gate on the matrix (Ubuntu + macOS + Windows) plus
`cargo-deny`, `cargo-audit`, `dependency-review` on PRs, and OpenSSF
Scorecard on pushes to `main`.

## PR process

1. Branch from `main`; PRs target `main`.
2. Keep changes minimal and task-scoped.
3. Justify any new dependency in the PR description.
4. Don't commit secrets, credentials, `.env` files, logs, private keys,
   local configs, hardcoded local paths, usernames, or private IPs.
5. Add or update tests for behavior changes.
6. Sign your commits if possible (`git commit -S`) — helps with
   provenance, not required.
7. Make sure CI is green before requesting review.

## Reporting bugs / requesting features

Use the issue templates:

- **Bug report** — reproduction steps, expected vs actual, environment.
- **Feature request** — motivation, proposed API, alternatives considered.

Security issues: **do not** open a public issue. Use GitHub's private
vulnerability reporting (see [SECURITY.md](SECURITY.md)).

## License

By contributing, you agree your contributions are licensed under
[the project's LICENSE](LICENSE) (default: AGPL-3.0-only).
