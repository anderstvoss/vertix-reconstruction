# Hardening Checklist

One-page tickable list for bootstrapping a new Rust repo to the same
posture as this one. Each item links to the file or doc section that
spells out the why and how. For full rationale, see
[REPO-SETUP.md](REPO-SETUP.md).

## Phase 1 — Repo files (copy from this repo)

Drop these into the new repo at the same paths. SHAs already pinned;
do not unpin without a deliberate audit.

### Source-tree configuration

- [ ] [.gitignore](../.gitignore) — Rust artifacts, env files,
      credentials, logs, OS/editor noise, coverage, build dirs, SBOM
      output.
- [ ] [.env.example](../.env.example) — placeholder for local config;
      block on `.env` is in the hook.
- [ ] [LICENSE](../LICENSE) — AGPL-3.0 by default; adjust here AND in
      `Cargo.toml [package].license`.
- [ ] [Cargo.toml](../Cargo.toml) — fill `description`, `repository`,
      `readme`, `keywords`, `categories`, `rust-version` (MSRV),
      `authors`, `[lints.rust]` (`unsafe_code = "forbid"`; a workspace
      with FFI crates sets `deny` and puts `#![forbid(unsafe_code)]` at
      every non-FFI crate root), `[lints.clippy]` (`all` + `pedantic`
      warn, priority -1; `undocumented_unsafe_blocks = "deny"`).
- [ ] [ruff.toml](../ruff.toml) — if the repo carries Python helpers:
      pyflakes + bandit rules, enforced by the `ruff-check` hook.
- [ ] [rust-toolchain.toml](../rust-toolchain.toml) — pin channel +
      `rustfmt`, `clippy` components for reproducible builds.
- [ ] [deny.toml](../deny.toml) — cargo-deny allow-lists for
      licenses + sources; advisories `version = 2`, yanked = deny.
- [ ] [CHANGELOG.md](../CHANGELOG.md) — Keep-a-Changelog 1.1.0
      skeleton with `[Unreleased]` section.

### Local hooks

- [ ] [.pre-commit-config.yaml](../.pre-commit-config.yaml) — frozen
      SHAs for gitleaks + pre-commit-hooks; local pygrep blockers for
      env files, private keys, local paths, private IPs, cloud URIs,
      binary artifacts; cargo fmt/check/clippy/test at commit stage;
      cargo deny + cargo audit at pre-push stage.
- [ ] [.githooks/pre-commit](../.githooks/pre-commit),
      [.githooks/pre-push](../.githooks/pre-push) — committed wrappers;
      the latter runs gitleaks (full HEAD history) + tracked-file
      blocker + local-paths guard, then hands off to pre-commit's
      pre-push stage.

### Community / process files

- [ ] [README.md](../README.md) — public-audience intro, license,
      setup, dev gates, contributing pointer.
- [ ] [SECURITY.md](../SECURITY.md) — vulnerability reporting +
      defensive posture summary.
- [ ] [CONTRIBUTING.md](../CONTRIBUTING.md) — dev setup, required
      gates, PR process, bug/feature/security reporting.
- [ ] [AGENTS.md](../AGENTS.md) — optional, for AI-agent collaborators.

### GitHub configuration

- [ ] [.github/CODEOWNERS](../.github/CODEOWNERS) — maintainer
      ownership of `.github/`, security docs, dependency manifests.
- [ ] [.github/dependabot.yml](../.github/dependabot.yml) — weekly
      cargo + actions updates.
- [ ] [.github/PULL_REQUEST_TEMPLATE.md](../.github/PULL_REQUEST_TEMPLATE.md)
      — PR checklist.
- [ ] [.github/ISSUE_TEMPLATE/bug_report.md](../.github/ISSUE_TEMPLATE/bug_report.md),
      [feature_request.md](../.github/ISSUE_TEMPLATE/feature_request.md),
      [config.yml](../.github/ISSUE_TEMPLATE/config.yml) — issue
      intake; blank issues disabled; security contact link.

### CI workflows (all visibility-gated)

- [ ] [.github/workflows/ci.yml](../.github/workflows/ci.yml) —
      `rust-lint`, `msrv`, `rust-test` (ubuntu+macos+windows), `policy`
      (matrix), `supply-chain`, `dependency-review`, `python-scripts`.
      harden-runner egress block on every Linux job.
- [ ] [.github/workflows/codeql.yml](../.github/workflows/codeql.yml)
      — GitHub-native code scanning for Rust and for the workflow files
      themselves (`actions`); `queries: security-extended`.
- [ ] [.github/workflows/scorecard.yml](../.github/workflows/scorecard.yml)
      — OpenSSF Scorecard; full triggers; `publish_results: true`;
      `id-token: write`.
- [ ] [.github/workflows/gitleaks-history.yml](../.github/workflows/gitleaks-history.yml)
      — full-history scan on every PR + push + weekly cron + dispatch.
- [ ] [.github/workflows/sbom.yml](../.github/workflows/sbom.yml) —
      CycloneDX SBOM on every push to main; uploaded as 90-day
      artifact.

### Operator helpers

- [ ] [scripts/deep-scan.sh](../scripts/deep-scan.sh) — ad-hoc full
      all-refs gitleaks. Run before any publish or release tag.
- [ ] [docs/REPO-SETUP.md](REPO-SETUP.md) — keep as-is; the blueprint
      itself is one of the files copied into the new repo.
- [ ] [docs/pre-commit-hooks.md](pre-commit-hooks.md) — developer
      reference for the hook stack.

## Phase 2 — Local clone setup

For each clone, once after cloning:

- [ ] `cargo install cargo-deny cargo-audit`
- [ ] `git config core.hooksPath .githooks`
- [ ] `rm -f .git/hooks/pre-commit .git/hooks/pre-push` (remove stale
      wrappers from any prior `pre-commit install`)
- [ ] `pipx install pre-commit` (or `pip install --user pre-commit`)
- [ ] `pre-commit run --all-files` → all commit-stage hooks pass
- [ ] `pre-commit run --all-files --hook-stage pre-push` → cargo-deny
      + cargo-audit pass

## Phase 3 — GitHub server-side (after going public)

Every job in every workflow is `if: ${{ !github.event.repository.private }}`
— they sit dormant on private and activate the moment the repo flips.

- [ ] **Before anything else:** run
      [scripts/deep-scan.sh](../scripts/deep-scan.sh) and confirm it
      passes. Flipping visibility publishes every reachable commit, so
      the full-history scan must happen while the repo is still private;
      the CI history scan only activates after the flip.
- [ ] Rename the repo to drop any `-private` suffix:
      `gh repo rename <new-name>` from inside the clone.
- [ ] Update [.github/ISSUE_TEMPLATE/config.yml](../.github/ISSUE_TEMPLATE/config.yml)
      security advisories URL to match the new name.
- [ ] Update `Cargo.toml [package].repository` if needed.
- [ ] (Optional) Rename the local clone directory:
      `cd .. && mv <old> <new>`. Then re-set `origin`:
      `git remote set-url origin git@github.com:<owner>/<new>.git`.
      Required for the Claude Code per-project memory symlink (path
      slug embeds the directory name) — see below.
- [ ] (Optional) Migrate the agent-memory symlink to the new local
      path slug if you renamed the directory:
      ```bash
      OLD="$HOME/.claude/projects/-home-$USER-Projects-<old>/memory"
      NEW="$HOME/.claude/projects/-home-$USER-Projects-<new>/memory"
      [ -L "$OLD" ] && rm "$OLD"
      ln -s "$PWD/.agents/memory" "$NEW"
      ```
- [ ] Flip visibility to public:
      `gh repo edit --visibility public --accept-visibility-change-consequences`.
- [ ] Run the server-side controls block from
      [REPO-SETUP.md](REPO-SETUP.md#step-3--enable-server-side-controls):
  - [ ] Secret scanning + push protection
  - [ ] Private vulnerability reporting
  - [ ] Dependabot alerts + automated security updates
  - [ ] Branch protection with required-status contexts
  - [ ] Tag protection ruleset for `v*`
  - [ ] Actions hardening: read-only default `GITHUB_TOKEN`, Actions
        may not approve PRs, first-time fork contributors need approval
- [ ] Open a draft PR to trigger the now-active CI; verify all
      required-status contexts pass.
- [ ] Manually `gh workflow run scorecard.yml --repo $REPO`; verify
      SARIF lands in Security tab within ~5 min.
- [ ] Manually `gh workflow run gitleaks-history.yml --repo $REPO`;
      verify it goes green.
- [ ] Manually `gh workflow run sbom.yml --repo $REPO`; verify the
      `sbom-cyclonedx-<sha>` artifact appears.
- [ ] Sanity test push protection: try pushing a fake AWS access key
      on a scratch branch. Push should be rejected server-side.

## Phase 4 — Optional follow-ups (defer until you have a real consumer)

- [ ] `cargo-vet` initial audit — declares trust of every dep.
      High initial cost; high blueprint signal. Recommended once the
      crate has any real dependencies.
- [ ] Signed commits / DCO enforcement — provenance for community
      contributions. Adds friction; skip for solo / small-team
      projects.
- [ ] Release-asset SBOM attachment — extend
      [sbom.yml](../.github/workflows/sbom.yml) with a
      `softprops/action-gh-release` step (SHA needs proper vetting)
      to attach `bom.json` to the GitHub release when the first
      tagged release lands.
- [ ] Bump action SHAs — periodically diff against upstream tags and
      re-pin.

## Phase 5 — Maintenance cadence

- Weekly: review Dependabot PRs; merge security updates immediately.
- Monthly: bump Rust toolchain in [rust-toolchain.toml](../rust-toolchain.toml)
  alongside any clippy sweep needed to clear new pedantic findings.
- Quarterly: re-run [scripts/deep-scan.sh](../scripts/deep-scan.sh)
  manually outside the CI cron; bump action SHAs.
- Before any release tag: deep-scan, verify CHANGELOG entry, generate
  SBOM, attach to release.
