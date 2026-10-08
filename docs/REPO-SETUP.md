# Maximally Hardened Public Rust Repo — Reusable Blueprint

End-to-end procedure for bootstrapping a single public GitHub repo
with the strongest free-tier hardening surface. Apply this file to
any new project that wants the same posture — copy it verbatim and
follow the checklists.

For the tickable one-page version, see
[HARDENING-CHECKLIST.md](HARDENING-CHECKLIST.md).

> **Why single-repo?** GitHub Free **public** repos unlock secret
> scanning, push protection, private vulnerability reporting,
> `actions/dependency-review-action`, OpenSSF Scorecard with token
> scope, branch protection / Repository Rulesets — all gated behind
> GHAS (paid) on private repos. Going public is the cheapest path to
> strong server-side controls. The split this repo used to maintain
> (a private working repo plus a public mirror) added complexity
> without adding security; the sync tooling was extracted to a
> separate toolkit reusable for projects that genuinely need an
> embargoed or sensitive parallel.

---

## Hardening Reference

| Control | Mechanism |
|---|---|
| Pre-commit gitleaks + custom blockers (env files, keys, local paths, private IPs, cloud URIs, binary artifacts) | `.pre-commit-config.yaml`, invoked via committed `.githooks/pre-commit` |
| Pre-push gitleaks + tracked-file blocker + local-paths guard + `cargo deny` + `cargo audit` | `.githooks/pre-push` + `.pre-commit-config.yaml` pre-push hooks |
| Full CI matrix: `cargo fmt`, `clippy -D warnings`, `cargo check`, `cargo test` on Ubuntu + macOS + Windows | `.github/workflows/ci.yml` `rust-lint`, `rust-test` jobs |
| Python helper lint (pyflakes + bandit via ruff) and unit tests | `ruff.toml` + `ruff-check` hook; `.github/workflows/ci.yml` `python-scripts` job |
| CodeQL `security-extended` for Rust and GitHub Actions workflows | `.github/workflows/codeql.yml` |
| Full pre-commit policy replay in CI on the same matrix | `.github/workflows/ci.yml` `policy` job |
| Supply chain audit (`cargo-deny` + `cargo-audit`) | `.github/workflows/ci.yml` `supply-chain` job |
| `actions/dependency-review-action` on PRs, `fail-on-severity: moderate` | `.github/workflows/ci.yml` `dependency-review` job |
| All third-party actions SHA-pinned | every workflow file |
| `step-security/harden-runner` egress block + explicit allowlist on every Linux job | every workflow file |
| OpenSSF Scorecard with SARIF publish + `id-token: write` | `.github/workflows/scorecard.yml` |
| Weekly full-history gitleaks scan across every ref | `.github/workflows/gitleaks-history.yml` |
| Ad-hoc deep scan helper | `scripts/deep-scan.sh` |
| Secret scanning + push protection (server-side) | `gh api` setup, public-repo only |
| Private vulnerability reporting | `gh api` setup, public-repo only |
| Dependabot alerts + automated security updates | `.github/dependabot.yml` + `gh api` enable |
| Branch protection / Rulesets with required-status checks | `gh api` setup |
| Codeowner review required on sensitive paths | `.github/CODEOWNERS` |
| AGPL-3.0 license | `LICENSE` + `Cargo.toml` `[package].license` |
| Clippy `pedantic` + `undocumented_unsafe_blocks = "deny"`; `unsafe_code` denied workspace-wide and forbidden at every non-FFI crate root | `Cargo.toml` `[lints.*]`, crate-root attributes |
| Read-only default `GITHUB_TOKEN`; Actions cannot approve PRs; `v*` tags protected | `gh api` setup |

---

## File Inventory

| Path | Purpose |
|---|---|
| `.pre-commit-config.yaml` | All commit-stage hooks + cargo-deny / cargo-audit at pre-push stage |
| `.githooks/pre-commit`, `.githooks/pre-push` | Generic wrappers — no machine paths |
| `.github/CODEOWNERS` | Maintainer ownership of `.github/`, security docs, dependency manifests |
| `.github/dependabot.yml` | Weekly cargo + GHA updates |
| `.github/PULL_REQUEST_TEMPLATE.md` | PR checklist |
| `.github/ISSUE_TEMPLATE/{bug_report.md, feature_request.md, config.yml}` | Issue intake; blank issues disabled; security contact link |
| `.github/workflows/ci.yml` | rust-lint, rust-test (matrix), policy (matrix), supply-chain, dependency-review |
| `.github/workflows/scorecard.yml` | OpenSSF Scorecard, full triggers, publish_results |
| `.github/workflows/gitleaks-history.yml` | Weekly main-history + all-refs gitleaks |
| `SECURITY.md` | Vulnerability reporting + defensive posture summary |
| `CONTRIBUTING.md` | Dev setup, required gates, PR process |
| `deny.toml` | cargo-deny license/source allowlist |
| `Cargo.toml` / `Cargo.lock` | Package metadata + lockfile (lockfile committed) |
| `LICENSE` | AGPL-3.0 (or your choice — adjust `Cargo.toml` to match) |
| `docs/REPO-SETUP.md` | This file |
| `AGENTS.md` | Optional AI-agent rules; safe to publish, useful to contributors |
| `scripts/deep-scan.sh` | Operator helper for ad-hoc full-history gitleaks |

---

## Bootstrap

Run from inside a freshly-created public GitHub repo (cloned to your
machine).

### Step 1 — Drop in the committed files

Copy every file from the inventory above into the new repo.

Make hook scripts executable:

```bash
chmod +x .githooks/pre-commit .githooks/pre-push scripts/deep-scan.sh
```

Verify the workflow + action SHAs match what this repo currently
ships — bumping pins is a deliberate, audited change.

### Step 2 — Local clone setup

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks

# Remove any stale wrappers from a prior `pre-commit install`.
rm -f .git/hooks/pre-commit .git/hooks/pre-push
```

`pre-commit` itself must be installed system-wide (via pipx or pip).
The committed wrappers in `.githooks/` invoke whichever `pre-commit`
is on `PATH`.

Verify:

```bash
git config core.hooksPath                            # → .githooks
ls .githooks/                                        # → pre-commit  pre-push
pre-commit run --all-files                           # all commit-stage hooks pass
pre-commit run --all-files --hook-stage pre-push     # cargo-deny + cargo-audit pass
```

### Step 3 — Enable server-side controls

```bash
REPO=<owner>/<repo>

# Secret scanning + push protection (free on public)
gh api --method PATCH /repos/$REPO --input - <<'EOF'
{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}
EOF

# Private vulnerability reporting
gh api -X PUT /repos/$REPO/private-vulnerability-reporting

# Dependabot alerts + automated security updates
gh api -X PUT /repos/$REPO/vulnerability-alerts
gh api -X PUT /repos/$REPO/automated-security-fixes

# Branch protection
cat > /tmp/branch-protection.json <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "Rust lint",
      "MSRV (cargo check on rust-version)",
      "Rust build and test (ubuntu-latest)",
      "Rust build and test (macos-latest)",
      "Rust build and test (windows-latest)",
      "Policy checks (ubuntu-latest)",
      "Policy checks (macos-latest)",
      "Policy checks (windows-latest)",
      "Supply chain audit",
      "Python script tests",
      "CodeQL (rust)",
      "CodeQL (actions)",
      "Gitleaks (full history)"
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "required_approving_review_count": 0,
    "require_code_owner_reviews": false,
    "dismiss_stale_reviews": false
  },
  "restrictions": null,
  "required_linear_history": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true
}
EOF
gh api -X PUT /repos/$REPO/branches/main/protection --input /tmp/branch-protection.json
```

For repos accepting community PRs, bump `required_approving_review_count`
to `1` and flip `require_code_owner_reviews` to `true`.

```bash
# Actions: read-only default token, no PR approval by workflows
gh api -X PUT /repos/$REPO/actions/permissions/workflow \
  -f default_workflow_permissions=read -F can_approve_pull_request_reviews=false

# Require approval before running workflows from first-time fork contributors
gh api -X PUT /repos/$REPO/actions/permissions/fork-pr-contributor-approval \
  -f approval_policy=first_time_contributors

# Protect release tags from update and deletion
gh api -X POST /repos/$REPO/rulesets --input - <<'JSON'
{"name":"release-tags","target":"tag","enforcement":"active",
 "conditions":{"ref_name":{"include":["refs/tags/v*"],"exclude":[]}},
 "rules":[{"type":"deletion"},{"type":"update"},{"type":"non_fast_forward"}]}
JSON
```

### Step 4 — First push

The pre-push hook will run gitleaks + tracked-file + local-paths
checks, then `cargo deny check` + `cargo audit`. All must pass before
the push is accepted.

Once pushed, CI runs the required-status contexts listed above plus
the PR-only `dependency-review` job (when triggered by a PR).

### Step 5 — Verify

```bash
REPO=<owner>/<repo>

gh api /repos/$REPO | jq '.security_and_analysis'
# → secret_scanning + secret_scanning_push_protection both "enabled"

gh api /repos/$REPO/branches/main/protection | jq '.required_status_checks.contexts'
# → returns every context from the list above

gh pr create --draft --title "verify ci" --body "noop"
# → opens a PR running the full required CI + dep-review

gh workflow run scorecard.yml --repo $REPO
# → SARIF appears in Security tab within ~5 min

gh workflow run gitleaks-history.yml --repo $REPO
# → completes green
```

Sanity test: try pushing a fake AWS access key on a scratch branch.
GitHub push protection should reject it server-side.

---

## Local Developer Setup (one-liner reference)

```bash
cargo install cargo-deny cargo-audit
git config core.hooksPath .githooks
rm -f .git/hooks/pre-commit .git/hooks/pre-push    # remove stale wrappers if any
```

Plus `pre-commit` itself installed system-wide:

```bash
pipx install pre-commit
# or:
pip install --user pre-commit
```

---

## Maintenance Notes

- **Dependabot updates** open PRs against `main`. Treat them like any
  other dep change: review the changelog, let CI run, merge.
- **Branch protection contexts must exactly match CI job names**
  (case-sensitive, including matrix expansion). If you add or rename
  a job, update the required-status-checks list via
  `gh api -X PUT .../branches/main/protection`.
- **The harden-runner allowlist** is a starter list. First real
  dependency that pulls from a new endpoint (e.g. a build-script
  fetching a vendored library) will fail with `blocked egress` — add
  the endpoint to the allowlist and re-run.
- **`required_linear_history`** on the public ruleset means PRs must
  be squashed or rebased — merge commits are blocked. Adjust if your
  workflow expects merge commits.
- **Deep scan before publish/merge.** Every-run hooks scan staged
  content and the current ref's history only; un-merged side branches
  and tags are covered weekly by `.github/workflows/gitleaks-history.yml`
  and ad-hoc by `scripts/deep-scan.sh`. Run the latter manually before
  any release tag.
- **Visibility-gated remote CI.** Every job in
  `.github/workflows/*.yml` carries `if: ${{ !github.event.repository.private }}`
  so the entire remote-CI surface is dormant while the repo is private
  and auto-activates on the flip to public. Rationale: pre-commit and
  pre-push hooks already cover the Linux gates locally; the matrix
  jobs and GHAS-gated jobs (`dependency-review`, Scorecard) only add
  value once public. To force-run a workflow on a private repo, use
  `gh workflow run <name>` — `workflow_dispatch` triggers still
  honour the gate, so test workflows by temporarily inverting the
  `if:` on a scratch branch instead.
- **Sensitive work fork.** If you ever need an embargoed parallel
  (security research, pre-disclosure fixes), the sync toolkit that
  used to live in this repo's `scripts/sync-*` is preserved as a
  separate private project (`repo-split-toolkit`). See its
  `docs/BOOTSTRAP.md` for the adoption recipe.
