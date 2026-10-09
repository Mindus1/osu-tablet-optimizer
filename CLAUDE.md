# osu-tablet-optimizer

Desktop app (Rust + iced) for any osu! player using OpenTabletDriver. It analyzes osu! replays (stable and lazer) and suggests tablet-area changes: size, aspect ratio, rotation. Suggestions are exported as OTD presets.

Design spec: `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Commands

```sh
cargo run
cargo test                                  # real-replay tests skip without tests/fixtures/
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo run --example update_tablets -- <OTD checkout>   # refresh src/core/tablets.tsv
```

## Architecture

- `src/lib.rs` exposes `core` as a library; `src/main.rs` is the iced binary on top of it.
- `src/core/`: pure logic, no iced. Replay parsing, beatmap index, hit extraction, aim fit, OTD settings, tablet table, persistence. Tests live here.
- `src/ui/`: iced pages and canvas charts. Thin: calls core, renders results.
- Flow: .osr + .osu → `hits` → `fit` → `otd` (area suggestion) → preset export.
- Units: osu!pixels (512×384) for replays and beatmaps, mm for the tablet area. Convert only in `fit`/`otd`.

## Code style

- General-purpose: no hardcoded user paths, tablets, names or resolutions. Everything machine-specific is detected or set in Setup.
- Clean code: small functions, intention-revealing names, one responsibility per module, no dead code.
- No speculative abstractions: no trait with one impl, no config for constants, no scaffolding "for later".
- Comment only the *why* the code can't say (math, file-format quirks). No doc comments that restate the signature, no banner comments, no commented-out code.
- Errors: `anyhow::Result` with `.context(..)` at I/O boundaries. Never `unwrap()` on user data.
- Add a dependency only when it replaces substantial code, and say why in the PR.
- Tests: TDD for `src/core/`. Math gets synthetic data with a known injected bias. Real-file tests read the gitignored `tests/fixtures/` and skip when it's missing. No GUI tests.

## Git workflow

Private repo `Mindus1/osu-tablet-optimizer`. `main` = releases only, `dev` = integration, `feature/<kebab-name>` branched from `dev`.
Commits: Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`, `ci:`).

### Feature

1. `git switch dev && git pull && git switch -c feature/<name>`
2. Implement with tests, committing in small logical steps.
3. `cargo fmt`, clippy, `cargo test`: all green.
4. Refactor only this feature's new code, then re-run the tests.
5. Check whether README.md needs updating (features, setup, usage) and update it in this branch.
6. `gh pr create --base dev`, then `gh pr merge --squash --delete-branch`.
7. Clean up: `git switch dev && git pull && git fetch --prune`. Delete local branches whose upstream is gone (`git branch -vv | grep ': gone]'`) and any remote `feature/*` branch without an open PR.
8. Ask the user: "Merge dev into main?" Every time. Never touch main without an explicit yes.

### Release (only after the user's yes)

1. Big refactor of the whole codebase (duplication, naming, module boundaries, dead code). Re-run tests.
2. Propose the bump with a one-line reason and wait for confirmation: patch = fixes only, minor = new features, major = milestone/breaking.
3. On `dev`: bump `version` in Cargo.toml, update README if needed, commit `chore: release vX.Y.Z`.
4. `gh pr create --base main --head dev`, then `gh pr merge --merge`.
5. On main: `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z` (`-m` is required because tag signing is enabled). The Release workflow builds Windows/Linux/macOS and publishes the GitHub release. Confirm with `gh run watch`.
6. `git switch dev && git merge --ff-only main && git push`.
