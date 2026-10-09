# osu-tablet-optimizer — Design

## Context

osu! tablet players tune their OpenTabletDriver (OTD) area by feel. This project is a general-purpose desktop app (Rust + iced) for **any osu! player with any OTD-supported tablet**. It analyzes their replays to find out whether their aim systematically overshoots or undershoots, is rotated, or is scaled differently on X and Y. It then suggests a better OTD tablet area (size, aspect ratio, rotation), and over time shows whether a new area actually helped. The project directory is empty, so this is a greenfield project. A `CLAUDE.md` with clean-code rules and a strict git workflow is part of the deliverable.

The author's machine is the **test environment only**, and nothing is tailored to it. It has OTD 0.6.7 portable (`Documents/Software/OpenTabletDriver.win-x64/userdata/`), a Wacom CTL-4100 with an 80×45 mm area in the top-left corner, about 6k stable replays, and a lazer store of 121k files / 58 GB.

## Decisions (from Q&A)

| Topic | Decision |
|---|---|
| osu! client | stable + lazer |
| "Tilt" | OTD area **rotation** (not pen tilt) |
| MVP analyses | over/undershoot (area size), rotation bias, per-axis scale (aspect). **Not** offset or heatmaps |
| Suggestion size | Full fit and capped increment side by side |
| Output | Export as a named **OTD preset** (`<OTD data>/Presets/<name>.json`). `settings.json` is never touched |
| Replay input | File picker / drag-drop, plus folder scan |
| Screen setup | Auto-read + confirm (MVP only needs the OTD area) |
| History | Yes. Per-config, tagged automatically from OTD snapshots, manual override |
| Charts | Error scatter, direction polar plot, area preview, history trend |
| Objects | Circles + slider heads, short movements filtered |
| Resize anchor | Keep centre, shifted minimally to stay inside the tablet |
| Clicks | Any press counts (K1/K2/M1/M2), so keyboard and pen-tap players both work |
| Player filter | Auto-detect the most frequent name after the first scan, editable list in Setup. Manually added files are never filtered |
| Tablet size | Bundled name → mm table generated from OTD's tablet configurations, matched against `settings.json` `Profiles[].Tablet`. Manual override for unknown tablets |
| OTD profiles | Profile picker when there are several tablets. Absolute mode only, with a clear notice otherwise. Supports the 0.6.x settings format (`Revision`) and warns on others |
| UI language | English only |
| Platforms | Windows + Linux + macOS |
| Layout | Sidebar tabs: Replays / Analysis / Area / History / Setup |
| Repo | `Mindus1/osu-tablet-optimizer`, **stays private** |
| Merges | feature→dev squash, dev→main merge commit |
| CI | PR checks (fmt/clippy/test, Linux) + 3-OS release build on tag |
| Versioning | SemVer, Cargo starts at 0.0.0, first release 0.1.0. Patch = fixes, minor = features, major = milestones |
| Test fixtures | Real replays in gitignored `tests/fixtures/`. Tests skip when absent |

## How the analysis works

For each matched hit `i`, `p` is the cursor at the previous hit, `o` the object centre and `c` the cursor at the key press:
- intended move `u = o − p`, actual move `v = c − p`
- **Model:** `v = B·u + k·û + ε`. `B` (2×2) is the player's effective gain: scale per axis, rotation and shear. `k` is a constant along-direction term that absorbs "stop at the circle edge" undershoot. Edge-stopping doesn't grow with jump distance, while a wrong area size does, so the two separate. 5 unknowns, solved with linear least squares (hand-written 5×5 solve).
- **Filters:** `|u| ≥ 3r` (min distance in radii, configurable), `|c − o| ≤ 3r` (outliers), previous object is a circle (slider-end positions need slider duration; `ponytail:` comment), gap ≤ 1000 ms, objects within 3 px of a neighbour skipped (stacking).
- **To OTD parameters:** the current tablet→playfield linear map is `M = diag(Dw/w, Dh/h)·R(−θ)` (constant factor cancels). The corrected map is `M' = B⁻¹·M`. New `w' = w·|row_x(M)|/|row_x(M')|`, same for `h'`, and `θ'` comes from the row directions. Shear can't be expressed in OTD, so it's reported only. **OTD's rotation sign convention gets verified against the OTD 0.6.x source and pinned by a test.**
- **Confidence:** 200 bootstrap resamples over hits give 90% CIs (`ponytail:` hit-level bootstrap, cluster by replay if CIs look too tight). A parameter whose CI covers "no change" is shown as *keep*.
- **Capped suggestion:** clamp `|w'/w − 1|`, `|h'/h − 1|` ≤ 3% and `|θ' − θ|` ≤ 2° (configurable).
- **Mods:** HR flips Y and scales CS/OD; EZ scales CS/OD. Lazer extras come from the .osr trailer JSON (MR axis flip, DA CS/OD). Replays with any mod outside a whitelist of visual/timing-only mods are skipped with a reason (AP, RX, lazer Wiggle/Transform/Magnetised/Barrel Roll, …). Radius `r = 54.4 − 4.48·CS`. Hit window `±(200 − 10·OD)` ms.
- **Presses:** rising edge of key A (K1|M1) or key B (K2|M2). Each object takes the first unused press inside its window.

## Architecture

Single crate. `core/` is pure logic with no iced dependency and is where all tests live. `ui/` is a thin layer over core.

```
src/main.rs            iced bootstrap
src/app.rs             State, Message, update, sidebar routing
src/core/replay.rs     .osr: header-only read (fast listing) + LZMA frames + lazer trailer mods
src/core/library.rs    find stable/lazer installs, index beatmaps (md5, path, title) + replay headers, JSON cache
src/core/hits.rs       mods, press↔object matching, samples (u, v, r)
src/core/fit.rs        regression, OTD-parameter decomposition, bootstrap, caps
src/core/otd.rs        locate OTD data dir, profiles, read area, write preset (keep centre + clamp)
src/core/tablets.rs    lookup in bundled tablets.tsv (include_str!)
src/core/history.rs    area snapshots, per-replay metrics, JSON persistence
src/core/config.rs     app settings (paths, player names, tablet size override, thresholds)
src/ui/{replays,analysis,area,history,setup}.rs
src/ui/charts.rs       canvas: scatter, polar, area preview, trend
examples/update_tablets.rs   regenerates src/core/tablets.tsv from an OTD checkout
```

**Dependencies:** `iced` (latest, 0.14.x at research time, `canvas` feature), `rfd` (file dialogs), `lzma-rs` (pure Rust, so release builds on all 3 OSes need no C toolchain), `rosu-map`, `md-5`, `serde` + `serde_json`, `dirs`, `chrono` (Windows ticks → local time), `anyhow`. The .osr reader is our own (~80 lines) instead of `rosu-replay`: listing thousands of replays needs a header-only read, and the lazer trailer has to be parsed anyway.

**Tablet table:** `cargo run --example update_tablets -- <OTD checkout>` walks `OpenTabletDriver.Configurations/Configurations/**/*.json` and writes `Name \t Specifications.Digitizer.Width \t Height` (mm) to `src/core/tablets.tsv`. The file is committed and refreshed by hand when OTD adds tablets. The README credits OTD (LGPL-3.0) as the source.

**Data discovery (general, with every path overridable in Setup):**
- OTD: `<platform data dir>/OpenTabletDriver` (`%LOCALAPPDATA%`, `$XDG_CONFIG_HOME` or `~/.config`, `~/Library/Application Support`). For portable installs, the user picks the OTD folder once, and the app accepts either the exe folder (uses `userdata/`) or the data folder itself.
- stable: `%LOCALAPPDATA%/osu!` → `Songs/`, `Data/r/`, `Replays/` (on Linux/macOS through Wine, picked by hand).
- lazer: `%APPDATA%/osu`, `~/.local/share/osu` or `~/Library/Application Support/osu`, honouring the `storage.ini` custom-location override → `files/`, `exports/`.
- One scan walks every root and keeps `.osu`, `.osr` and extension-less files (lazer's store). It reuses cache entries whose modified time and size are unchanged, and classifies the rest by content: `osu file format v`, or a `.osr` header signature (mode byte 0–3, plausible version, `0x0b 0x20` + 32-char MD5). Changed files are read in parallel. On the test machine: 171k candidates, 25 s first scan, 1.5 s rescan.
- Persistence lives in `dirs::data_dir()/osu-tablet-optimizer/`: `config.json`, `history.json`, `library.json` (file cache: path → modified, size, beatmap md5 + title or replay header).

**OTD preset export:** parse the settings as `serde_json::Value`, change only the selected profile's `AbsoluteModeSettings.Tablet.{Width,Height,X,Y,Rotation}` (plus `LockAspectRatio=false` if the aspect changes), and write `Presets/oto-YYYY-MM-DD-{capped|full}.json`. Unknown fields are preserved.

**History tagging:** at launch and before each scan, compare the OTD area with the last snapshot. On a change, log `{area, first_seen = settings.json mtime}`. A replay belongs to the latest snapshot with `first_seen ≤ replay timestamp`; older replays go under "unknown". Manual reassignment is stored per replay. Per replay the history keeps: n, gain, aspect, rotation bias, RMS error/r.

## GUI pages

- **Replays**: Scan (progress), drag-drop, "Add files…". Table with date, player, map, mods, accuracy and config. Filters: last session / 7 d / 30 d / all, config, and text. Checkbox selection, then "Analyze selected". Shows at most 500 rows (`ponytail:` virtualize if needed). "Assign config…" for manual override.
- **Analysis**: movement-aligned error scatter (unit circle = radius), 16-sector polar plot (gain + angular error per direction), key numbers with CIs, list of skipped replays with reasons.
- **Area**: tablet outline with current, full-fit (dashed) and capped (solid) areas, a numbers table and "Export preset".
- **History**: snapshot list (renamable), trend chart with a metric selector, points coloured by config.
- **Setup**: detected paths with override, OTD profile picker, tablet name + size (from the table, with override), player names (auto-detected), thresholds/caps. On first run the app opens here and shows what was detected.

## Roadmap (one feature branch → PR to `dev` each; each starts with a `writing-plans` plan for just that feature)

1. `feature/app-shell`: Cargo project, sidebar + empty pages, CI + release workflows, README build section
2. `feature/replay-parser`: `replay.rs` + tests
3. `feature/beatmap-index`: `library.rs` (stable + lazer, incremental, replay discovery, player auto-detect)
4. `feature/hit-extraction`: `hits.rs` (mods, matching, filters)
5. `feature/aim-fit`: `fit.rs` (synthetic recovery tests)
6. `feature/otd-settings`: `otd.rs` + `tablets.rs` + `update_tablets` example (discovery, profiles, preset write, rotation convention test)
7. `feature/replays-page`: Replays + Setup pages, `config.rs` (its first consumer)
8. `feature/analysis-page`: scatter + polar + metrics
9. `feature/area-page`: area preview + export, the **suggested first release (0.1.0)**
10. `feature/history`: snapshots, tagging, History page

## Verification

- `cargo test`: synthetic recovery tests inject a gain (0.95), rotation (+2°) and aspect (1.03), and the fit must recover them within tolerance. The OTD decomposition round-trips. Preset JSON keeps unknown fields. Tablet lookup finds known names and returns `None` for unknown ones. Real-fixture tests parse a stable and a lazer replay and match hits against the .osu.
- Manual end to end (`cargo run`) on the test machine, which exercises the non-default paths: first run opens Setup → stable + lazer detected, OTD not found at the default path → pick the portable folder → tablet resolved from the table → scan → player auto-detected → filter the last 30 days → Analyze → plausible scatter/polar output → Export preset → the preset appears in OTD's Presets menu and loads → play → re-analyze → History shows the new config as a separate trend series.
- Release: a pushed tag produces 3 OS artifacts on the GitHub release.

## Later (out of MVP)

Area offset (X/Y) and tablet heatmaps (need the osu! resolution and playfield mapping), physical pen tilt, live folder watching, osu! API downloads, movements that start at slider ends, pen-tap landing-drift analysis.
