# TEMPLATE_PROJECT_NAME

One-sentence description of what this project is.

> **Status:** early WIP / alpha / stable — pick one. Link to
> [CHANGELOG.md](CHANGELOG.md) for tracked changes.

**Before doing anything else with this repo, read
[READ_ME_FIRST.md](READ_ME_FIRST.md).** That file documents how to
customise the template for your project (name, language, crate type,
license, owner) and the one-time bootstrap procedure.

## License

[AGPL-3.0-only](LICENSE) (template default; adjust here and in
`Cargo.toml`'s `[package].license` if you pick something else).

## Setup

After cloning, run once:

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks

# If you previously ran `pre-commit install` on this clone, remove the
# now-stale wrappers in .git/hooks/ so git only consults .githooks/:
rm -f .git/hooks/pre-commit .git/hooks/pre-push
```

`core.hooksPath` redirects git to the committed `.githooks/`
directory. The `pre-commit` wrapper delegates to the `pre-commit`
Python package (install via pipx or pip — see
<https://pre-commit.com/#install>); the `pre-push` wrapper runs the
custom safety checks (gitleaks, tracked-file blocker, local-paths
scan) and then hands off to pre-commit's pre-push-stage hooks
(`cargo deny` + `cargo audit`).

## Development

```bash
cargo build
cargo test
```

Local gates (also run in CI on every PR):

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
gitleaks detect
ruff check .    # only if the repo carries Python helpers under scripts/
```

Before publishing or merging anything that matters, run a deep
gitleaks scan across all branches and tags:

```bash
scripts/deep-scan.sh
```

Record user-visible changes in [`CHANGELOG.md`](CHANGELOG.md) under
the `Unreleased` section as part of any feature, fix, or breaking
change.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Security

See [SECURITY.md](SECURITY.md). For the end-to-end hardening
procedure (reusable across projects), see
[`docs/REPO-SETUP.md`](docs/REPO-SETUP.md); for the tickable
one-page bootstrap list, see
[`docs/HARDENING-CHECKLIST.md`](docs/HARDENING-CHECKLIST.md).
