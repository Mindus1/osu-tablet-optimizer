# Deferred Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** fix the deferred minors from features 6–10 that a user actually notices:
1. **Stable intervals:** the same selection gives the same intervals every run (the bootstrap saw paths in `HashSet` order).
2. **Older-area warning:** the Area page says when analyzed replays were played with an older logged area, since the suggestion corrects the current one.
3. **Linux Artist Mode** counts as absolute (its output path is `…Output.LinuxArtistMode`, without "Absolute").
4. **Stale export message:** a new analysis clears the Area page's last export message.

**Architecture:** 1 sorts paths in `aim::analyze`. 2 adds `History::from_other_areas` and one note on the Area page. 3 is the `absolute` check in `otd::profile`. 4 resets `app.area` when an analysis finishes.

**Tech Stack:** Rust, iced 0.14.

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- TDD for `src/core/`; no GUI tests.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- **Unknown replays** (before the first logged area) don't trigger the older-area note: on first use most replays are unknown, and they were probably played with the current area. Pinned by `replays_from_older_areas_are_counted`.

## Left out on purpose

Cosmetic items the UI redesign covers (decimal places, the skipped list's cut-off, stale notices), and rare ones: an "unknown" area filter, Assign re-tagging hidden rows, non-UTF-8 paths in `history.json`, a history that can't be set aside, partial preset writes, one malformed OTD profile failing all, the same area on another profile.

---

### Task 1: Core fixes

**Files:** `src/core/aim.rs`, `src/core/history.rs`, `src/core/otd.rs`

- [ ] **Step 1: Failing tests.**

`aim.rs`:

```rust
    #[test]
    fn replays_are_read_in_path_order() {
        let paths = [PathBuf::from("b.osr"), PathBuf::from("a.osr")];

        let report = analyze(&paths, &HashMap::new(), MAPPING, &Settings::default());

        let order: Vec<&PathBuf> = report.skipped.iter().map(|(path, _)| path).collect();
        assert_eq!(order, [&paths[1], &paths[0]]);
    }
```

(`MAPPING` becomes a test-module const shared with `unreadable_replays_are_listed_with_their_reason`.)

`history.rs`:

```rust
    #[test]
    fn replays_from_older_areas_are_counted() {
        let mut history = two_areas();
        history.assign([PathBuf::from("assigned.osr")], Some(0));
        let replays = [
            (PathBuf::from("unknown.osr"), entry(50, 0.0, 30)),
            (PathBuf::from("old.osr"), entry(150, 0.0, 30)),
            (PathBuf::from("assigned.osr"), entry(250, 0.0, 30)),
            (PathBuf::from("current.osr"), entry(250, 0.0, 30)),
        ];

        assert_eq!(history.from_other_areas(&replays), 2);
    }
```

`otd.rs`:

```rust
    #[test]
    fn linux_artist_mode_is_absolute() {
        let settings = SETTINGS.replace(
            "OpenTabletDriver.Desktop.Output.RelativeMode",
            "OpenTabletDriver.Desktop.Output.LinuxArtistMode",
        );
        let (_dir, settings) = installed("otd-artist", &settings);

        assert!(settings.profiles[1].absolute);
    }
```

- [ ] **Step 2:** `cargo test --lib` → the aim test fails on order, the others don't compile / fail.
- [ ] **Step 3: Implement.**

`aim::analyze` starts with

```rust
    // Sorted so the bootstrap sees the replays in the same order, and the intervals come out the
    // same, every time the same selection is analyzed.
    let mut paths = paths.to_vec();
    paths.sort();
```

`History`:

```rust
    // How many of these replays were played with a logged area other than the current (last)
    // one. Unknown ones don't count: they were probably played with the current area.
    pub fn from_other_areas(&self, replays: &[(PathBuf, Entry)]) -> usize {
        let current = self.snapshots.len().checked_sub(1);
        replays
            .iter()
            .filter(|(path, entry)| {
                self.snapshot_of(path, entry.played_at_ms)
                    .is_some_and(|area| Some(area) != current)
            })
            .count()
    }
```

`otd::profile`:

```rust
        // Linux Artist Mode maps the area absolutely too.
        absolute: value["OutputMode"]["Path"]
            .as_str()
            .is_some_and(|path| path.contains("Absolute") || path.contains("ArtistMode")),
```

- [ ] **Step 4:** `cargo test` → all pass. Commit `fix: stable intervals, older-area count, Linux Artist Mode`.

### Task 2: UI

**Files:** `src/ui/area.rs`, `src/ui/analysis.rs`

- [ ] Area page notes, after the "Kept…" note:

```rust
    let elsewhere = app
        .analysis
        .last
        .as_ref()
        .map_or(0, |analysis| app.history.from_other_areas(&analysis.report.measured));
    if elsewhere > 0 {
        notes = notes.push(text(format!(
            "{elsewhere} of the analyzed replays were played with an older area. The suggestion \
             corrects your current area, so filter the Replays page to it and analyze again."
        )));
    }
```

- [ ] `analysis::update`, `Message::Done`: `app.area = area::State::default();` (the last export message belongs to the previous analysis).
- [ ] Gate, commit `fix: Area page notes older-area replays and drops stale export messages`, PR, CI, review, merge.
