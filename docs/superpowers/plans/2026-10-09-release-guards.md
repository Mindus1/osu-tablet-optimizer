# Release Guards Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A release can only be published when its tag matches `Cargo.toml`'s version, and every GitHub action runs on its current major version (no Node 20 deprecation warnings).

**Architecture:** A cheap `verify-version` job (Ubuntu, seconds) runs first in `release.yml`, and `build` depends on it. A mismatched tag therefore fails before any Windows or macOS minutes are spent. Pre-release tags compare their base version: `v0.1.0-rc1` is valid for `0.1.0`, and the throwaway `v0.0.0-ci-test` is valid for `0.0.0`. Action majors are bumped in both workflows.

**Tech Stack:** GitHub Actions, bash.

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`. This plan is user-requested release hardening (the deferred minors from feature 1) and doesn't change the spec.

## Global Constraints

- Branch `feature/release-guards` from `dev`. Squash-merge into `dev`.
- Action versions (latest releases checked on 2026-10-09): `actions/checkout@v7`, `actions/upload-artifact@v7`, `actions/download-artifact@v8`. `Swatinem/rust-cache@v2` and `dtolnay/rust-toolchain@stable` are already current.
- Their release notes have no breaking changes for this usage (download by name with `merge-multiple`, upload by name + glob, plain checkout).
- Commits: Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- **A tag that doesn't match `Cargo.toml`:** the run must fail in `verify-version`, and no build job may start. Pinned by the RED tag run in Task 3.
- **A pre-release tag of the current version** (`v0.0.0-ci-test` with Cargo `0.0.0`): it must pass and publish as a pre-release. Pinned by the GREEN tag run in Task 3.
- **Version lookup picking up a dependency's `version =` line:** `[package]` comes first in `Cargo.toml` and the line is anchored (`^version = `), so the first match is the package version. Checked in Task 1 Step 2.
- **New artifact action majors silently changing paths:** the GREEN run must still publish exactly the three assets with the same names. Pinned in Task 3.
- **Node 20 deprecation annotations:** expected none on the GREEN run. Pinned in Task 3.

---

### Task 1: Tag/version guard

**Files:**
- Modify: `.github/workflows/release.yml`, `CLAUDE.md` (Release step 5)

**Interfaces:**
- Consumes: `release.yml` from feature 1 (jobs `build`, `publish`).
- Produces: job `verify-version`. `build` gets `needs: verify-version`.

- [ ] **Step 1: Add the job**

In `.github/workflows/release.yml`, insert at the start of `jobs:` (before `build:`):

```yaml
  verify-version:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - name: Tag must match Cargo.toml version
        run: |
          version=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
          tag="${GITHUB_REF_NAME#v}"
          if [ "${tag%%-*}" != "$version" ]; then
            echo "::error::Tag $GITHUB_REF_NAME does not match Cargo.toml version $version"
            exit 1
          fi

```

and add `needs: verify-version` to `build`, directly under `build:`:

```yaml
  build:
    needs: verify-version
    strategy:
```

- [ ] **Step 2: Check the guard logic locally (RED, then GREEN)**

```bash
guard() { version=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2); tag="${1#v}"; [ "${tag%%-*}" = "$version" ] && echo PASS || echo FAIL; }
guard v9.9.9-ci-test; guard v0.0.1; guard v0.0.0-ci-test; guard v0.0.0
```

Expected: `FAIL FAIL PASS PASS` (Cargo version is `0.0.0`).

- [ ] **Step 3: Document it in CLAUDE.md**

In `CLAUDE.md` Release step 5, append this sentence after "(`-m` is required because tag signing is enabled).":

```markdown
 The workflow refuses tags whose version differs from `Cargo.toml` (pre-release suffixes like `-rc1` are allowed).
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml CLAUDE.md
git commit -m "ci: refuse release tags that don't match Cargo.toml

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Bump action versions

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`

**Interfaces:**
- Consumes: both workflows.
- Produces: the same jobs, on current action majors.

- [ ] **Step 1: Replace the versions**

```bash
sed -i 's/actions\/checkout@v5/actions\/checkout@v7/; s/actions\/upload-artifact@v4/actions\/upload-artifact@v7/; s/actions\/download-artifact@v4/actions\/download-artifact@v8/' .github/workflows/ci.yml .github/workflows/release.yml
grep -n 'uses:' .github/workflows/*.yml
```

Expected: only `checkout@v7`, `upload-artifact@v7`, `download-artifact@v8`, `dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2` remain.

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml
git commit -m "ci: bump actions to current major versions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Prove it on GitHub and merge

**Files:** none new.

**Interfaces:**
- Consumes: Tasks 1–2 on `feature/release-guards`.
- Produces: the guarded release workflow merged into `dev`.

- [ ] **Step 1: Push and open the PR, CI green**

```bash
git push -u origin feature/release-guards
gh pr create --base dev --title "ci: release tag guard, current action versions" --body "Release refuses tags that don't match Cargo.toml; actions bumped to current majors.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks --watch
```

Expected: `check` passes, with no Node 20 annotation on it.

- [ ] **Step 2: RED — a mismatched tag fails before building**

```bash
git tag -a v9.9.9-ci-test -m "v9.9.9-ci-test" && git push origin v9.9.9-ci-test
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
gh run view "$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')" --json jobs -q '.jobs[] | .name + " " + .conclusion'
```

If `gh run list` returns nothing yet, re-run that line. Expected: `verify-version failure`, and every `build (...)` job `skipped`. Clean up: `git push origin :refs/tags/v9.9.9-ci-test && git tag -d v9.9.9-ci-test`.

- [ ] **Step 3: GREEN — a matching pre-release tag publishes**

```bash
git tag -a v0.0.0-ci-test -m "v0.0.0-ci-test" && git push origin v0.0.0-ci-test
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')" --exit-status
gh release view v0.0.0-ci-test --json isPrerelease,assets -q '.isPrerelease, [.assets[].name]'
```

Expected: the run succeeds; `true`; the assets are `osu-tablet-optimizer-linux-x64.tar.gz`, `osu-tablet-optimizer-macos-arm64.tar.gz` and `osu-tablet-optimizer-windows-x64.exe`. Check the job annotations with `gh api repos/Mindus1/osu-tablet-optimizer/check-runs/<job id>/annotations`: there should be no Node 20 warning. Clean up: `gh release delete v0.0.0-ci-test --cleanup-tag --yes`.

- [ ] **Step 4: Squash-merge and clean up**

```bash
gh pr merge --squash --delete-branch
git switch dev && git pull && git fetch --prune
git branch -vv | grep ': gone]' || true
```

- [ ] **Step 5: Ask the user**

"Merge dev into main?" Wait for an explicit answer.
