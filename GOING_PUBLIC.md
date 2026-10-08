# Going Public Checklist

Actions to take **before** flipping this repository from private to public on GitHub.

## 1. Secrets & history hygiene

- [ ] Run `scripts/deep-scan.sh` (gitleaks over every branch and tag) and confirm it passes. This is a hard gate: the CI history scan only starts after the flip, when it is too late.
- [ ] Rotate any credential that ever touched this repo, even if it was removed in a later commit. History is forever once public.
- [ ] Confirm `.env`, `.env.*` (except `.env.example`), private keys, and local config files are gitignored and absent from history.
- [ ] If anything sensitive is found in history, rewrite with `git filter-repo` (or BFG) and force-push **before** going public.

## 2. Repository metadata

- [ ] Set a clear repo description and homepage URL on GitHub.
- [ ] Add topics/tags so the project is discoverable.
- [ ] Confirm `LICENSE` is correct and the copyright holder/year are accurate.
- [ ] Update `README.md`: project status, install/build/run instructions, badges (CI, license, crates.io if applicable).
- [ ] Verify `CONTRIBUTING.md`, `SECURITY.md`, and `CHANGELOG.md` are current and link from `README.md`.
- [ ] Add a `CODE_OF_CONDUCT.md` if one isn't already in place.

## 3. Security disclosure

- [ ] Confirm `SECURITY.md` lists a reachable contact (email or GitHub security advisory link).
- [ ] Enable **Private vulnerability reporting** under Settings → Code security.
- [ ] Enable Dependabot alerts, Dependabot security updates, and secret scanning (with push protection) — all are free on public repos.
- [ ] Enable CodeQL / code scanning if applicable.

## 4. CI, branch protection & permissions

- [ ] Review `.github/workflows/*` — remove any internal-only steps, registries, or self-hosted runners.
- [ ] Audit workflow `permissions:` blocks — default to `contents: read` and grant more only where needed.
- [ ] Confirm no workflow secrets are echoed or written to logs.
- [ ] For `pull_request_target` or workflows that check out untrusted code: review carefully or remove.
- [ ] Set branch protection on `main`: required reviews, required status checks, linear history, signed commits if desired.
- [ ] Restrict who can push tags / create releases (`v*` tag ruleset in `docs/REPO-SETUP.md`).
- [ ] Actions settings: default `GITHUB_TOKEN` read-only, Actions may not approve PRs, first-time fork contributors need approval (commands in `docs/REPO-SETUP.md`).
- [ ] Add every new required check (including `Python script tests` and `CodeQL (actions)`) to branch protection; names must match job names exactly.

## 5. Code & dependency review

- [ ] Run `cargo deny check` and resolve any advisories, license, or source issues (`deny.toml` is already in the repo).
- [ ] Run `cargo audit` (or equivalent) and address open advisories.
- [ ] Grep the codebase for `TODO`, `FIXME`, `HACK`, internal hostnames, internal usernames, ticket IDs, or stack traces that shouldn't be public.
- [ ] Confirm all dependencies are OSI-licensed and compatible with this project's license.

## 6. Issues, PRs, and discussions

- [ ] Close or triage any stale issues/PRs you don't want visible.
- [ ] Decide whether to enable Discussions, Issues, Projects, and Wiki — disable the ones you won't maintain.
- [ ] Add issue templates and a PR template under `.github/` if not already present.

## 7. Releases & artifacts

- [ ] Tag a clean baseline release (e.g. `v0.1.0`) so external users have a known-good starting point.
- [ ] If publishing to crates.io / a package registry, reserve the name and verify `Cargo.toml` metadata (`description`, `repository`, `license`, `readme`, `keywords`, `categories`).

## 8. The flip

- [ ] Settings → General → Danger Zone → **Change repository visibility** → Public.
- [ ] Immediately re-verify Dependabot, secret scanning, and code scanning are enabled (some settings reset on visibility change).
- [ ] Watch the repo yourself and announce only after the above is green.

## 9. Post-public

- [ ] Monitor the Security tab for the first 24–48 hours.
- [ ] Respond to the first wave of issues/PRs promptly to set the tone for contributors.
- [ ] Schedule a recurring review (quarterly) of dependencies, advisories, and stale issues.
