# Replays & Setup Pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the first usable screens. **Setup** detects and edits the osu! folders, the OpenTabletDriver folder and profile, the tablet size, the players and the analysis limits, and saves them. **Replays** lists your osu!standard replays with filters, selection, "Add files…", drag-and-drop and live scan progress.

**Architecture:**
- **Core (tested), `storage`:** app folder (`dirs::data_dir()/osu-tablet-optimizer`) and `write_atomic` (temp file + rename).
- **Core, `config`:** a `Config` saved as JSON. `None` and empty fields mean "detect".
- **Core, `listing`:** replay rows with filters: last session (an hour-long pause starts a new one), 7 days, 30 days, all, and a title search; plus rows for picked or dropped files.
- **Core, `library` additions:** a cache version (an outdated cache rebuilds), atomic save, a `scan` progress callback reported every 1,000 files, and `roots_for(picked)`. A picked stable folder expands like a detected one, and a lazer folder follows its `storage.ini`.
- **UI (thin, not unit-tested), `app`:** owns the state and runs load cache → scan → save cache on a background thread. Progress reaches the UI through `iced::stream::channel`. File drops come from `event::listen_with`.
- **UI, `ui::replays` and `ui::setup`:** views plus update functions over `&mut App`. The first run opens Setup and auto-selects the most frequent player once the first scan finishes.

**Verified with a prototype of exactly this code on the test machine:**
- **Tests:** 113 pass and clippy is clean.
- **First run:** the scan saved `library.json` (50 MB), and `config.json` got `players: ["Mindus"]`.
- **Replays page:** shows 92 replays from the last 30 days with dates, titles, mods and accuracy.
- **Setup page:** shows both osu! roots. Picking the portable OTD folder resolves to `userdata`, the profile is "Wacom CTL-4100", and the size line reads "152 × 95 mm, from OpenTabletDriver's tablet list". Screenshots were taken of the app window only (`PrintWindow`).

**Tech Stack:** iced 0.14, `rfd` 0.17 (async native dialogs; `default-features = false, features = ["xdg-portal"]` so Linux builds need no system libraries), `chrono` 0.4 (local time for dates).

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- `src/core/` is pure logic and tested. `src/ui/` and `src/app.rs` are thin, with no GUI tests (CLAUDE.md).
- General-purpose: nothing machine-specific is hardcoded; everything comes from detection or Setup.
- Settings and cache are written atomically. User data (OTD `settings.json`, osu! files) is only read.
- Commits: Conventional Commits ending with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR
  ```

## Review Focus

- **A config from an older or newer version** (missing fields): loads with defaults. Pinned by `missing_fields_take_defaults` (Task 1).
- **A cache written by an older version** of the app: loads empty and rebuilds instead of showing stale titles. Pinned by `outdated_cache_loads_empty` (Task 2).
- **Players with different letter case** ("me" vs "ME"): treated as the same player. Pinned by `rows_are_newest_first_for_the_chosen_players` (Task 3).
- **A dropped replay of another mode** (taiko, catch, mania): rejected with a message, never listed. Pinned by `other_modes_are_not_listed` (Task 3).
- **Scan progress while thousands of files change:** the UI gets bounded updates (every 1,000 files plus the last). Pinned by `scan_reports_progress_up_to_the_total` (Task 2).

---

### Task 1: App storage and config

**Files:**
- Modify: `src/core.rs`, `src/core/fit.rs` (serde on `Settings`)
- Create: `src/core/storage.rs`, `src/core/config.rs`

**Interfaces:**
- Produces:
  - `storage::app_dir() -> Option<PathBuf>` and `storage::write_atomic(&Path, &[u8]) -> Result<()>`.
  - `config::Config { osu_roots: Option<Vec<PathBuf>>, otd_dir: Option<PathBuf>, players: Vec<String>, profile: usize, tablet_size: Option<(f64, f64)>, fit: fit::Settings }` with `load(&Path) -> Result<Option<Config>>` (`None` on first run), `save`, `osu_roots()` and `otd_dir()`.
  - `fit::Settings` gains `Serialize, Deserialize` and `#[serde(default)]`.

- [ ] **Step 1:** In `src/core.rs` add `pub mod config;` and `pub mod storage;` (keep the list alphabetical). Apply to `src/core/fit.rs`:

```diff
index 4509ea9..25c1c0f 100644
--- a/src/core/fit.rs
+++ b/src/core/fit.rs
@@ -1,3 +1,5 @@
+use serde::{Deserialize, Serialize};
+
 use crate::core::hits::Sample;
 
 // ponytail: fixed statistics knobs; expose them in Setup if users ask.
@@ -10,7 +12,8 @@ const TAIL: f64 = 0.05;
 
 type Mat2 = [[f64; 2]; 2];
 
-#[derive(Debug, Clone, Copy, PartialEq)]
+#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
+#[serde(default)]
 pub struct Settings {
     pub min_jump_radii: f64,
     pub max_error_radii: f64,
```

- [ ] **Step 2: Write the failing tests.** Create `src/core/storage.rs` and `src/core/config.rs` containing only their `#[cfg(test)] mod tests` blocks from Step 4.
- [ ] **Step 3:** Run `cargo test --lib core::config core::storage`. Expected: compile errors (`Config`, `write_atomic` missing).
- [ ] **Step 4: Implement.** Final `src/core/storage.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn app_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("osu-tablet-optimizer"))
}

// Writes next to the target and renames over it, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;

    #[test]
    fn write_atomic_creates_and_replaces() {
        let dir = TempDir::new("storage");
        let path = dir.0.join("nested/config.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(!path.with_extension("tmp").exists());
    }
}
```

Final `src/core/config.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::{fit, library, otd, storage};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // None: detect the osu! installs.
    pub osu_roots: Option<Vec<PathBuf>>,
    // None: OpenTabletDriver's default data folder.
    pub otd_dir: Option<PathBuf>,
    // Empty: every player's replays are shown.
    pub players: Vec<String>,
    pub profile: usize,
    // None: the size from the bundled tablet table.
    pub tablet_size: Option<(f64, f64)>,
    pub fit: fit::Settings,
}

impl Config {
    // None on first run, before anything was saved.
    pub fn load(path: &Path) -> Result<Option<Config>> {
        if !path.exists() {
            return Ok(None);
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(config))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        storage::write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn osu_roots(&self) -> Vec<PathBuf> {
        self.osu_roots
            .clone()
            .unwrap_or_else(library::default_roots)
    }

    pub fn otd_dir(&self) -> Option<PathBuf> {
        self.otd_dir
            .as_deref()
            .map(otd::resolve_data_dir)
            .or_else(otd::default_data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;

    #[test]
    fn config_survives_save_and_load() {
        let dir = TempDir::new("config");
        let path = dir.0.join("config.json");
        let config = Config {
            osu_roots: Some(vec![PathBuf::from("D:/osu!")]),
            players: vec!["someone".to_string()],
            profile: 1,
            tablet_size: Some((152.0, 95.0)),
            ..Config::default()
        };

        config.save(&path).unwrap();

        assert_eq!(Config::load(&path).unwrap(), Some(config));
    }

    #[test]
    fn first_run_has_no_config() {
        let dir = TempDir::new("config-missing");

        assert_eq!(Config::load(&dir.0.join("config.json")).unwrap(), None);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let dir = TempDir::new("config-partial");
        let path = dir.write("config.json", r#"{ "players": ["someone"] }"#);

        let config = Config::load(&path).unwrap().unwrap();

        assert_eq!(config.players, ["someone"]);
        assert_eq!(config.fit, fit::Settings::default());
        assert_eq!(config.osu_roots, None);
    }

    #[test]
    fn corrupt_config_is_an_error() {
        let dir = TempDir::new("config-corrupt");
        let path = dir.write("config.json", "{ not json");

        assert!(Config::load(&path).is_err());
    }
}
```

- [ ] **Step 5:** Run `cargo test --lib core::config core::storage`. Expected: 5 passed. Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- [ ] **Step 6: Commit** `feat: app storage folder and saved config`.

---

### Task 2: Library cache version, atomic save, scan progress, picked folders

**Files:**
- Modify: `src/core/library.rs`, `src/core/hits.rs`, `src/core/fit.rs` (ignored tests call the new `scan`)

**Interfaces:**
- `Library::scan(&mut self, roots: &[PathBuf], progress: &(dyn Fn(usize, usize) + Sync)) -> Vec<PathBuf>`. `progress(done, total)` covers the changed files being read: every 1,000 and at the end.
- `Library::save` writes `{"version": 1, "files": …}` atomically, and `load` returns an empty library for any other version.
- `pub fn roots_for(picked: &Path) -> Vec<PathBuf>`.
- The test `TempDir::roots` becomes `pub(crate)`.

- [ ] **Step 1: Write the failing tests.** Add `outdated_cache_loads_empty`, `scan_reports_progress_up_to_the_total` and `picked_folders_expand_like_detected_installs` (shown in the diff below) to `mod tests` in `src/core/library.rs`.
- [ ] **Step 2:** Run `cargo test --lib core::library`. Expected: compile errors (`roots_for` missing, `scan` takes 1 argument).
- [ ] **Step 3: Implement.** Apply to `src/core/library.rs`:

```diff
index 2451837..863545b 100644
--- a/src/core/library.rs
+++ b/src/core/library.rs
@@ -1,8 +1,9 @@
 use std::collections::HashMap;
 use std::fs::{self, File};
-use std::io::{BufRead, BufReader, BufWriter, Read, Write};
+use std::io::{BufRead, BufReader, Read};
 use std::num::NonZero;
 use std::path::{Path, PathBuf};
+use std::sync::atomic::{AtomicUsize, Ordering};
 use std::thread;
 use std::time::SystemTime;
 
@@ -11,9 +12,13 @@ use rosu_map::section::metadata::Metadata;
 use serde::{Deserialize, Serialize};
 
 use crate::core::replay::{self, Header};
+use crate::core::storage;
 
 const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
 const BEATMAP_SIGNATURE: &[u8] = b"osu file format v";
+// Bump when cached content changes meaning (e.g. how titles are built), so old caches rebuild.
+const CACHE_VERSION: u32 = 1;
+const PROGRESS_STEP: usize = 1000;
 
 #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
 enum Content {
@@ -35,34 +40,49 @@ pub struct Beatmap<'a> {
     pub title: &'a str,
 }
 
-#[derive(Debug, Default, Serialize, Deserialize)]
+#[derive(Debug, Default)]
 pub struct Library {
     files: HashMap<PathBuf, Entry>,
 }
 
+#[derive(Serialize, Deserialize)]
+struct Cache<Files> {
+    #[serde(default)]
+    version: u32,
+    files: Files,
+}
+
 impl Library {
+    // A missing or outdated cache loads empty, so the next scan rebuilds it.
     pub fn load(path: &Path) -> Result<Library> {
         if !path.exists() {
             return Ok(Library::default());
         }
         let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
-        serde_json::from_reader(BufReader::new(file))
-            .with_context(|| format!("reading {}", path.display()))
+        let cache: Cache<HashMap<PathBuf, Entry>> =
+            serde_json::from_reader(BufReader::new(file))
+                .with_context(|| format!("reading {}", path.display()))?;
+        if cache.version != CACHE_VERSION {
+            return Ok(Library::default());
+        }
+        Ok(Library { files: cache.files })
     }
 
     pub fn save(&self, path: &Path) -> Result<()> {
-        if let Some(dir) = path.parent() {
-            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
-        }
-        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
-        let mut writer = BufWriter::new(file);
-        serde_json::to_writer(&mut writer, self)?;
-        writer.flush()?;
-        Ok(())
+        let cache = Cache {
+            version: CACHE_VERSION,
+            files: &self.files,
+        };
+        storage::write_atomic(path, &serde_json::to_vec(&cache)?)
     }
 
     // Returns the files that could not be read or cached; the next scan retries them.
-    pub fn scan(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
+    // `progress(done, total)` is called from the reading threads as changed files are read.
+    pub fn scan(
+        &mut self,
+        roots: &[PathBuf],
+        progress: &(dyn Fn(usize, usize) + Sync),
+    ) -> Vec<PathBuf> {
         let mut candidates = Vec::new();
         for root in roots {
             collect_candidates(root, &mut candidates);
@@ -83,7 +103,16 @@ impl Library {
             }
         }
 
-        let contents = par_map(&changed, |(path, _, _)| read_content(path));
+        let total = changed.len();
+        let done = AtomicUsize::new(0);
+        let contents = par_map(&changed, |(path, _, _)| {
+            let content = read_content(path);
+            let count = done.fetch_add(1, Ordering::Relaxed) + 1;
+            if count.is_multiple_of(PROGRESS_STEP) || count == total {
+                progress(count, total);
+            }
+            content
+        });
         for ((path, modified, len), content) in changed.into_iter().zip(contents) {
             match content {
                 Ok(content) => {
@@ -177,6 +206,18 @@ pub fn lazer_root(default: &Path) -> PathBuf {
         .map_or_else(|| default.to_path_buf(), PathBuf::from)
 }
 
+// A folder the user picked: stable (has osu!.exe) and lazer (has client.realm or storage.ini)
+// folders expand like detected ones; anything else is scanned as it is.
+pub fn roots_for(picked: &Path) -> Vec<PathBuf> {
+    if picked.join("osu!.exe").is_file() {
+        stable_roots(picked)
+    } else if picked.join("client.realm").is_file() || picked.join("storage.ini").is_file() {
+        vec![lazer_root(picked)]
+    } else {
+        vec![picked.to_path_buf()]
+    }
+}
+
 fn songs_dir(stable: &Path) -> PathBuf {
     let newest_user_cfg = fs::read_dir(stable)
         .into_iter()
@@ -326,7 +367,7 @@ pub(crate) mod tests {
             path
         }
 
-        fn roots(&self) -> &[PathBuf] {
+        pub(crate) fn roots(&self) -> &[PathBuf] {
             std::slice::from_ref(&self.0)
         }
     }
@@ -351,7 +392,7 @@ pub(crate) mod tests {
 
     fn scanned(dir: &TempDir) -> Library {
         let mut library = Library::default();
-        assert!(library.scan(dir.roots()).is_empty());
+        assert!(library.scan(dir.roots(), &|_, _| {}).is_empty());
         library
     }
 
@@ -417,6 +458,29 @@ pub(crate) mod tests {
         assert_eq!(stable_roots(&dir.0), [dir.0.clone(), elsewhere]);
     }
 
+    #[test]
+    fn picked_folders_expand_like_detected_installs() {
+        let dir = TempDir::new("picked");
+        let elsewhere = std::env::temp_dir().join("oto-picked-songs");
+        dir.write("stable/osu!.exe", "");
+        dir.write(
+            "stable/osu!.me.cfg",
+            format!("BeatmapDirectory = {}\n", elsewhere.display()),
+        );
+        dir.write("lazer/storage.ini", "FullPath = D:\\osu-data\n");
+        dir.write("other/readme.txt", "");
+
+        assert_eq!(
+            roots_for(&dir.0.join("stable")),
+            [dir.0.join("stable"), elsewhere]
+        );
+        assert_eq!(
+            roots_for(&dir.0.join("lazer")),
+            [PathBuf::from("D:\\osu-data")]
+        );
+        assert_eq!(roots_for(&dir.0.join("other")), [dir.0.join("other")]);
+    }
+
     #[test]
     fn indexes_osu_files_by_md5_with_title() {
         let dir = TempDir::new("osu-files");
@@ -538,7 +602,7 @@ pub(crate) mod tests {
         fs::write(&path, BEATMAP.replace("Hard", "Easy")).unwrap();
         set_modified(&path, modified);
 
-        library.scan(dir.roots());
+        library.scan(dir.roots(), &|_, _| {});
 
         assert!(
             library
@@ -556,7 +620,7 @@ pub(crate) mod tests {
         fs::write(&path, &edited).unwrap();
         set_modified(&path, SystemTime::now() + Duration::from_secs(5));
 
-        library.scan(dir.roots());
+        library.scan(dir.roots(), &|_, _| {});
 
         let beatmaps = library.beatmaps_by_md5();
         assert!(beatmaps.contains_key(md5_hex(edited.as_bytes()).as_str()));
@@ -570,7 +634,7 @@ pub(crate) mod tests {
         let mut library = scanned(&dir);
         fs::remove_file(&path).unwrap();
 
-        library.scan(dir.roots());
+        library.scan(dir.roots(), &|_, _| {});
 
         assert!(library.files.is_empty());
     }
@@ -583,7 +647,11 @@ pub(crate) mod tests {
         link_dir(&songs.0, &stable.0.join("Songs"));
 
         let mut library = Library::default();
-        assert!(library.scan(&stable_roots(&stable.0)).is_empty());
+        assert!(
+            library
+                .scan(&stable_roots(&stable.0), &|_, _| {})
+                .is_empty()
+        );
 
         assert!(
             library
@@ -599,7 +667,7 @@ pub(crate) mod tests {
         set_modified(&path, SystemTime::UNIX_EPOCH - Duration::from_secs(60));
 
         let mut library = Library::default();
-        let unreadable = library.scan(dir.roots());
+        let unreadable = library.scan(dir.roots(), &|_, _| {});
 
         assert_eq!(unreadable, [path]);
         library.save(&dir.0.join("library.json")).unwrap();
@@ -614,7 +682,7 @@ pub(crate) mod tests {
         fs::write(&path, BEATMAP).unwrap();
 
         let mut library = Library::default();
-        let unreadable = library.scan(dir.roots());
+        let unreadable = library.scan(dir.roots(), &|_, _| {});
 
         assert_eq!(unreadable, [path]);
         library.save(&dir.0.join("library.json")).unwrap();
@@ -634,6 +702,33 @@ pub(crate) mod tests {
         assert_eq!(loaded.files, library.files);
     }
 
+    #[test]
+    fn outdated_cache_loads_empty() {
+        let dir = TempDir::new("old-cache");
+        dir.write("Songs/set/map.osu", BEATMAP);
+        let cache = dir.0.join("library.json");
+        scanned(&dir).save(&cache).unwrap();
+        let text = fs::read_to_string(&cache).unwrap();
+        fs::write(&cache, text.replace("\"version\":1", "\"version\":0")).unwrap();
+
+        assert!(Library::load(&cache).unwrap().files.is_empty());
+    }
+
+    #[test]
+    fn scan_reports_progress_up_to_the_total() {
+        let dir = TempDir::new("progress");
+        for i in 0..3 {
+            dir.write(&format!("Data/r/{i}.osr"), replay_bytes("player"));
+        }
+        let last = std::sync::Mutex::new((0, 0));
+
+        Library::default().scan(dir.roots(), &|done, total| {
+            *last.lock().unwrap() = (done, total)
+        });
+
+        assert_eq!(*last.lock().unwrap(), (3, 3));
+    }
+
     #[test]
     fn missing_cache_loads_empty() {
         let dir = TempDir::new("no-cache");
@@ -673,10 +768,10 @@ pub(crate) mod tests {
         let roots = default_roots();
         let mut library = Library::default();
         let started = std::time::Instant::now();
-        let unreadable = library.scan(&roots);
+        let unreadable = library.scan(&roots, &|_, _| {});
         let first = started.elapsed();
         let started = std::time::Instant::now();
-        library.scan(&roots);
+        library.scan(&roots, &|_, _| {});
         eprintln!(
             "{roots:?}: {} files, first scan {first:?}, rescan {:?}, {} unreadable, player {:?}",
             library.files.len(),
```

Then apply to `src/core/hits.rs` and `src/core/fit.rs`:

```diff
index c01b889..dad69e5 100644
--- a/src/core/hits.rs
+++ b/src/core/hits.rs
@@ -584,7 +584,7 @@ mod tests {
     #[ignore = "scans the osu! installs on this machine"]
     fn real_replays_land_near_their_objects() {
         let mut library = library::Library::default();
-        library.scan(&library::default_roots());
+        library.scan(&library::default_roots(), &|_, _| {});
         let beatmaps = library.beatmaps_by_md5();
         let mut replays: Vec<_> = library
             .replays()
--- a/src/core/fit.rs
+++ b/src/core/fit.rs
@@ -665,7 +668,7 @@ mod tests {
         use crate::core::{hits, library};
 
         let mut library = library::Library::default();
-        library.scan(&library::default_roots());
+        library.scan(&library::default_roots(), &|_, _| {});
         let beatmaps = library.beatmaps_by_md5();
         let mut replays: Vec<_> = library
             .replays()
```

- [ ] **Step 4:** Run `cargo test --lib core::`. Expected: all pass, including 3 new library tests. Then `cargo fmt --check && cargo clippy --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat: library scan progress, cache version and picked folders`.

---

### Task 3: Replay listing

**Files:**
- Modify: `src/core.rs`, `src/core/replay.rs` (`legacy_mods` public; test `Osr` gets `beatmap_md5`)
- Create: `src/core/listing.rs`

**Interfaces:**
- `listing::Period { LastSession, Week, Month (default), All }` with `ALL` and `Display`.
- `listing::Row { path, played_at_ms, player, title, mods, accuracy }`.
- `listing::rows(&Library, players: &[String], Period, search: &str, now_ms: i64) -> Vec<Row>`: osu!standard only, newest first, players matched ignoring case, empty means everyone.
- `listing::row_for_file(&Path, &HashMap<&str, library::Beatmap>) -> Result<Row>`.

- [ ] **Step 1:** Add `pub mod listing;` to `src/core.rs`. Apply to `src/core/replay.rs`:

```diff
index 66a97f0..00baf97 100644
--- a/src/core/replay.rs
+++ b/src/core/replay.rs
@@ -149,7 +149,7 @@ fn parse_frames(text: &str) -> Result<Vec<Frame>> {
     Ok(frames)
 }
 
-fn legacy_mods(mut bits: u32) -> Vec<Mod> {
+pub fn legacy_mods(mut bits: u32) -> Vec<Mod> {
     if bits & NIGHTCORE != 0 {
         bits &= !DOUBLE_TIME;
     }
@@ -262,6 +262,7 @@ pub(crate) mod tests {
     pub(crate) struct Osr<'a> {
         pub(crate) version: i32,
         pub(crate) player: &'a str,
+        pub(crate) beatmap_md5: &'a str,
         pub(crate) legacy_mods: u32,
         pub(crate) ticks: i64,
         pub(crate) frames: &'a str,
@@ -273,6 +274,7 @@ pub(crate) mod tests {
             Osr {
                 version: STABLE_VERSION,
                 player: "player",
+                beatmap_md5: "0123456789abcdef0123456789abcdef",
                 legacy_mods: 0,
                 ticks: TICKS_AT_UNIX_EPOCH,
                 frames: "0|256|192|0,",
@@ -285,7 +287,7 @@ pub(crate) mod tests {
         pub(crate) fn bytes(&self) -> Vec<u8> {
             let mut b = vec![0];
             b.extend(self.version.to_le_bytes());
-            push_string(&mut b, "0123456789abcdef0123456789abcdef");
+            push_string(&mut b, self.beatmap_md5);
             push_string(&mut b, self.player);
             push_string(&mut b, "replay-md5");
             for count in [300u16, 20, 3, 7, 5, 1] {
```

- [ ] **Step 2: Write the failing tests.** Create `src/core/listing.rs` with only its test module from Step 4.
- [ ] **Step 3:** Run `cargo test --lib core::listing`. Expected: compile errors.
- [ ] **Step 4: Implement.** Final `src/core/listing.rs`:

```rust
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::core::library::{self, Library};
use crate::core::replay::{self, Header};

const MS_PER_DAY: i64 = 86_400_000;
// A pause this long between two plays starts a new session.
const SESSION_GAP_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    LastSession,
    Week,
    #[default]
    Month,
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [
        Period::LastSession,
        Period::Week,
        Period::Month,
        Period::All,
    ];
}

impl std::fmt::Display for Period {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Period::LastSession => "Last session",
            Period::Week => "Last 7 days",
            Period::Month => "Last 30 days",
            Period::All => "All time",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub played_at_ms: i64,
    pub player: String,
    pub title: String,
    pub mods: String,
    pub accuracy: f64,
}

// osu!standard replays of the given players (all players when empty), newest first.
pub fn rows(
    library: &Library,
    players: &[String],
    period: Period,
    search: &str,
    now_ms: i64,
) -> Vec<Row> {
    let beatmaps = library.beatmaps_by_md5();
    let mut rows: Vec<Row> = library
        .replays()
        .filter(|(_, header)| header.mode == 0 && is_played_by(header, players))
        .map(|(path, header)| row(path, header, &beatmaps))
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.played_at_ms));

    let since = match period {
        Period::LastSession => session_start(&rows),
        Period::Week => now_ms - 7 * MS_PER_DAY,
        Period::Month => now_ms - 30 * MS_PER_DAY,
        Period::All => i64::MIN,
    };
    let search = search.to_lowercase();
    rows.retain(|row| row.played_at_ms >= since && row.title.to_lowercase().contains(&search));
    rows
}

// For replays the user picked or dropped in; those are never filtered by player.
pub fn row_for_file(path: &Path, beatmaps: &HashMap<&str, library::Beatmap<'_>>) -> Result<Row> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let header = replay::read_header(&mut BufReader::new(file))
        .with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        header.mode == 0,
        "{} is not an osu!standard replay",
        path.display()
    );
    Ok(row(path, &header, beatmaps))
}

fn is_played_by(header: &Header, players: &[String]) -> bool {
    players.is_empty()
        || players
            .iter()
            .any(|player| player.eq_ignore_ascii_case(&header.player))
}

fn session_start(newest_first: &[Row]) -> i64 {
    let mut start = newest_first
        .first()
        .map_or(i64::MIN, |row| row.played_at_ms);
    for row in newest_first {
        if start - row.played_at_ms > SESSION_GAP_MS {
            break;
        }
        start = row.played_at_ms;
    }
    start
}

fn row(path: &Path, header: &Header, beatmaps: &HashMap<&str, library::Beatmap<'_>>) -> Row {
    Row {
        path: path.to_path_buf(),
        played_at_ms: header.played_at_ms,
        player: header.player.clone(),
        title: beatmaps.get(header.beatmap_md5.as_str()).map_or_else(
            || "(beatmap not installed)".to_string(),
            |b| b.title.to_string(),
        ),
        mods: mods(header.legacy_mods),
        accuracy: accuracy(header),
    }
}

fn mods(bits: u32) -> String {
    let acronyms: Vec<String> = replay::legacy_mods(bits)
        .into_iter()
        .map(|m| m.acronym)
        .collect();
    if acronyms.is_empty() {
        "NM".to_string()
    } else {
        acronyms.concat()
    }
}

fn accuracy(header: &Header) -> f64 {
    let [great, ok, meh, miss] = [
        header.count_300,
        header.count_100,
        header.count_50,
        header.count_miss,
    ]
    .map(f64::from);
    let total = great + ok + meh + miss;
    if total == 0.0 {
        return 0.0;
    }
    (300.0 * great + 100.0 * ok + 50.0 * meh) / (300.0 * total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;
    use crate::core::replay::tests::Osr;

    const HOUR_MS: i64 = 60 * 60 * 1000;
    const NOW_MS: i64 = 1_800_000_000_000;
    const BEATMAP: &str =
        "osu file format v14\n\n[Metadata]\nTitle:Song\nArtist:Band\nVersion:Hard\n";

    fn ticks(played_at_ms: i64) -> i64 {
        621_355_968_000_000_000 + played_at_ms * 10_000
    }

    fn library_with(name: &str, replays: &[(&str, &str, i64)]) -> (TempDir, Library) {
        let dir = TempDir::new(name);
        let md5 = format!("{:x}", md5::compute(BEATMAP));
        dir.write("Songs/set/map.osu", BEATMAP);
        for (i, (name, player, played_at_ms)) in replays.iter().enumerate() {
            let bytes = Osr {
                player,
                beatmap_md5: &md5,
                ticks: ticks(*played_at_ms),
                ..Osr::default()
            }
            .bytes();
            dir.write(&format!("Data/r/{i}-{name}.osr"), bytes);
        }
        let mut library = Library::default();
        library.scan(dir.roots(), &|_, _| {});
        (dir, library)
    }

    fn players(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.player.as_str()).collect()
    }

    #[test]
    fn rows_are_newest_first_for_the_chosen_players() {
        let (_dir, library) = library_with(
            "listing-players",
            &[
                ("a", "me", NOW_MS - 3 * HOUR_MS),
                ("b", "someone else", NOW_MS - 2 * HOUR_MS),
                ("c", "ME", NOW_MS - HOUR_MS),
            ],
        );

        let rows = rows(&library, &["me".to_string()], Period::All, "", NOW_MS);

        assert_eq!(players(&rows), ["ME", "me"]);
        assert_eq!(rows[0].title, "Band - Song [Hard]");
    }

    #[test]
    fn no_players_means_everyone() {
        let (_dir, library) = library_with(
            "listing-everyone",
            &[("a", "me", NOW_MS), ("b", "you", NOW_MS)],
        );

        assert_eq!(rows(&library, &[], Period::All, "", NOW_MS).len(), 2);
    }

    #[test]
    fn last_session_stops_at_an_hour_long_pause() {
        let (_dir, library) = library_with(
            "listing-session",
            &[
                ("old", "a", NOW_MS - 10 * HOUR_MS),
                ("start", "b", NOW_MS - 2 * HOUR_MS),
                ("middle", "c", NOW_MS - HOUR_MS - 30 * 60 * 1000),
                ("end", "d", NOW_MS - HOUR_MS),
            ],
        );

        let rows = rows(&library, &[], Period::LastSession, "", NOW_MS);

        assert_eq!(players(&rows), ["d", "c", "b"]);
    }

    #[test]
    fn week_and_month_count_back_from_now() {
        let (_dir, library) = library_with(
            "listing-period",
            &[
                ("recent", "a", NOW_MS - 2 * MS_PER_DAY),
                ("weeks", "b", NOW_MS - 20 * MS_PER_DAY),
                ("old", "c", NOW_MS - 40 * MS_PER_DAY),
            ],
        );

        assert_eq!(
            players(&rows(&library, &[], Period::Week, "", NOW_MS)),
            ["a"]
        );
        assert_eq!(
            players(&rows(&library, &[], Period::Month, "", NOW_MS)),
            ["a", "b"]
        );
        assert_eq!(rows(&library, &[], Period::All, "", NOW_MS).len(), 3);
    }

    #[test]
    fn search_matches_titles_ignoring_case() {
        let (_dir, library) = library_with("listing-search", &[("a", "me", NOW_MS)]);

        assert_eq!(
            rows(&library, &[], Period::All, "band - SONG", NOW_MS).len(),
            1
        );
        assert!(rows(&library, &[], Period::All, "other", NOW_MS).is_empty());
    }

    #[test]
    fn rows_show_mods_and_accuracy() {
        let (dir, library) = library_with("listing-mods", &[]);
        let path = dir.write(
            "Replays/hddt.osr",
            Osr {
                legacy_mods: (1 << 3) | (1 << 6),
                ..Osr::default()
            }
            .bytes(),
        );

        let row = row_for_file(&path, &library.beatmaps_by_md5()).unwrap();

        assert_eq!(row.mods, "HDDT");
        assert!((row.accuracy - 92_150.0 / 97_200.0).abs() < 1e-12);
        assert_eq!(row.title, "(beatmap not installed)");
        assert_eq!(mods(0), "NM");
    }

    #[test]
    fn other_modes_are_not_listed() {
        let (dir, mut library) = library_with("listing-modes", &[]);
        let mut bytes = Osr::default().bytes();
        bytes[0] = 3;
        let path = dir.write("Data/r/mania.osr", bytes);
        library.scan(dir.roots(), &|_, _| {});

        assert!(rows(&library, &[], Period::All, "", NOW_MS).is_empty());
        assert!(row_for_file(&path, &library.beatmaps_by_md5()).is_err());
    }
}
```

- [ ] **Step 5:** Run `cargo test --lib core::listing`. Expected: 7 passed. Then the fmt and clippy gate.
- [ ] **Step 6: Commit** `feat: replay listing with periods, sessions and search`.

---

### Task 4: Setup and Replays pages

**Files:**
- Modify: `Cargo.toml` (`rfd`, `chrono`), `src/main.rs`, `src/app.rs`
- Create: `src/ui.rs`, `src/ui/replays.rs`, `src/ui/setup.rs`

There are no unit tests (CLAUDE.md: no GUI tests). Verification is a build, clippy, and running the app.

- [ ] **Step 1: Dependencies.** Run `cargo add chrono@0.4` and `cargo add rfd@0.17 --no-default-features --features xdg-portal`.
- [ ] **Step 2: Wiring.** Apply to `src/main.rs`:

```diff
index 624a377..aa776a8 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,13 +1,15 @@
 #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
 
 mod app;
+mod ui;
 
 use app::App;
 
 fn main() -> iced::Result {
-    iced::application(App::default, App::update, App::view)
+    iced::application(App::new, App::update, App::view)
         .title("osu! Tablet Optimizer")
         .theme(App::theme)
+        .subscription(App::subscription)
         .window_size((1200.0, 800.0))
         .run()
 }
```

Create `src/ui.rs`:

```rust
pub mod replays;
pub mod setup;
```

- [ ] **Step 3: App state, background scan, file drops.** Final `src/app.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use iced::futures::{SinkExt, StreamExt};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Subscription, Task, Theme, event, window};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::otd::OtdSettings;
use osu_tablet_optimizer::core::storage;

use crate::ui::{replays, setup};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Replays,
    Analysis,
    Area,
    History,
    Setup,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Replays,
        Page::Analysis,
        Page::Area,
        Page::History,
        Page::Setup,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Replays => "Replays",
            Page::Analysis => "Analysis",
            Page::Area => "Area",
            Page::History => "History",
            Page::Setup => "Setup",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Page),
    Rescan,
    Scan(ScanEvent),
    FileDropped(PathBuf),
    Replays(replays::Message),
    Setup(setup::Message),
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Progress(usize, usize),
    Done {
        library: Arc<Library>,
        unreadable: usize,
        error: Option<String>,
    },
}

pub struct App {
    pub page: Page,
    pub config: Config,
    first_run: bool,
    pub library: Arc<Library>,
    pub scanning: Option<(usize, usize)>,
    pub otd: Result<OtdSettings, String>,
    pub replays: replays::State,
    pub setup: setup::State,
    pub notice: Option<String>,
}

impl App {
    pub fn new() -> (App, Task<Message>) {
        let loaded = config_path().map(|path| Config::load(&path));
        let (config, first_run, notice) = match loaded {
            Some(Ok(Some(config))) => (config, false, None),
            Some(Ok(None)) | None => (Config::default(), true, None),
            Some(Err(error)) => (Config::default(), false, Some(format!("{error:#}"))),
        };
        let mut app = App {
            page: if first_run {
                Page::Setup
            } else {
                Page::Replays
            },
            otd: Err(String::new()),
            setup: setup::State::new(&config),
            config,
            first_run,
            library: Arc::default(),
            scanning: Some((0, 0)),
            replays: replays::State::default(),
            notice,
        };
        app.reload_otd();
        let scan = app.scan();
        (app, scan)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => self.page = page,
            Message::Rescan if self.scanning.is_none() => {
                self.scanning = Some((0, 0));
                return self.scan();
            }
            Message::Rescan => {}
            Message::Scan(ScanEvent::Progress(done, total)) => self.scanning = Some((done, total)),
            Message::Scan(ScanEvent::Done {
                library,
                unreadable,
                error,
            }) => {
                self.scanning = None;
                self.library = library;
                if self.first_run
                    && self.config.players.is_empty()
                    && let Some(player) = self.library.most_frequent_player()
                {
                    self.config.players = vec![player.to_string()];
                    self.setup = setup::State::new(&self.config);
                    self.save_config();
                }
                self.notice = error.or_else(|| {
                    (unreadable > 0).then(|| format!("{unreadable} files could not be read"))
                });
                self.replays.refresh(&self.library, &self.config);
            }
            Message::FileDropped(path) => replays::add_files(self, vec![path]),
            Message::Replays(message) => return replays::update(self, message),
            Message::Setup(message) => return setup::update(self, message),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let nav = Page::ALL.map(|page| {
            let style = if page == self.page {
                button::primary
            } else {
                button::text
            };
            button(page.label())
                .width(Fill)
                .style(style)
                .on_press(Message::Navigate(page))
                .into()
        });
        let sidebar = column(nav).spacing(4).padding(8).width(160);
        let page = match self.page {
            Page::Replays => replays::view(self),
            Page::Setup => setup::view(self),
            other => text(other.label()).size(24).into(),
        };
        let mut content = column![page].spacing(8);
        if let Some(notice) = &self.notice {
            content = content.push(text(notice).size(14));
        }

        row![
            sidebar,
            container(content).padding(16).width(Fill).height(Fill)
        ]
        .into()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _, _| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => {
                Some(Message::FileDropped(path))
            }
            _ => None,
        })
    }

    pub fn save_config(&mut self) {
        if let Some(Err(error)) = config_path().map(|path| self.config.save(&path)) {
            self.notice = Some(format!("{error:#}"));
        }
    }

    pub fn reload_otd(&mut self) {
        self.otd = match self.config.otd_dir() {
            Some(dir) => OtdSettings::load(&dir).map_err(|error| format!("{error:#}")),
            None => Err("OpenTabletDriver settings not found; choose its folder.".to_string()),
        };
    }

    // Loads the cached library, rescans every root on a background thread and saves the cache.
    pub fn scan(&self) -> Task<Message> {
        let roots = self.config.osu_roots();
        let cache = cache_path();
        Task::run(
            iced::stream::channel(16, async move |mut output| {
                let (sender, mut events) = iced::futures::channel::mpsc::unbounded();
                std::thread::spawn(move || {
                    let mut library = cache
                        .as_deref()
                        .and_then(|path| Library::load(path).ok())
                        .unwrap_or_default();
                    let unreadable = library.scan(&roots, &|done, total| {
                        let _ = sender.unbounded_send(ScanEvent::Progress(done, total));
                    });
                    let error = cache
                        .as_deref()
                        .and_then(|path| library.save(path).err())
                        .map(|error| format!("{error:#}"));
                    let _ = sender.unbounded_send(ScanEvent::Done {
                        library: Arc::new(library),
                        unreadable: unreadable.len(),
                        error,
                    });
                });
                while let Some(event) = events.next().await {
                    if output.send(event).await.is_err() {
                        break;
                    }
                }
            }),
            Message::Scan,
        )
    }
}

fn config_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("config.json"))
}

fn cache_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("library.json"))
}
```

- [ ] **Step 4: Replays page.** `src/ui/replays.rs`:

```rust
use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use iced::widget::{
    button, checkbox, column, pick_list, progress_bar, row, scrollable, space, text, text_input,
};
use iced::{Center, Element, Fill, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::listing::{self, Period, Row};

use crate::app::{self, App, Page};

// ponytail: a plain column of rows; switch to a virtualized list if 500 rows feel slow.
const MAX_ROWS: usize = 500;

#[derive(Debug, Default)]
pub struct State {
    period: Period,
    search: String,
    rows: Vec<Row>,
    added: Vec<Row>,
    pub selected: HashSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Period(Period),
    Search(String),
    Toggle(PathBuf, bool),
    ToggleAll(bool),
    AddFiles,
    FilesPicked(Vec<PathBuf>),
    Analyze,
}

impl State {
    pub fn refresh(&mut self, library: &Library, config: &Config) {
        let now_ms = Utc::now().timestamp_millis();
        self.rows = listing::rows(library, &config.players, self.period, &self.search, now_ms);
    }

    fn shown(&self) -> impl Iterator<Item = &Row> {
        self.added.iter().chain(&self.rows).take(MAX_ROWS)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    let state = &mut app.replays;
    match message {
        Message::Period(period) => {
            state.period = period;
            state.refresh(&app.library, &app.config);
        }
        Message::Search(search) => {
            state.search = search;
            state.refresh(&app.library, &app.config);
        }
        Message::Toggle(path, on) => select(&mut state.selected, path, on),
        Message::ToggleAll(on) => {
            let shown: Vec<PathBuf> = state.shown().map(|row| row.path.clone()).collect();
            for path in shown {
                select(&mut state.selected, path, on);
            }
        }
        Message::AddFiles => {
            let dialog = rfd::AsyncFileDialog::new()
                .add_filter("osu! replay", &["osr"])
                .pick_files();
            return Task::perform(dialog, |files| {
                let paths = files
                    .unwrap_or_default()
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect();
                app::Message::Replays(Message::FilesPicked(paths))
            });
        }
        Message::FilesPicked(paths) => add_files(app, paths),
        Message::Analyze => app.page = Page::Analysis,
    }
    Task::none()
}

// Picked or dropped replays are listed above the library and selected right away.
pub fn add_files(app: &mut App, paths: Vec<PathBuf>) {
    let beatmaps = app.library.beatmaps_by_md5();
    for path in paths {
        match listing::row_for_file(&path, &beatmaps) {
            Ok(row) => {
                app.replays.selected.insert(row.path.clone());
                if !app.replays.added.iter().any(|added| added.path == row.path) {
                    app.replays.added.push(row);
                }
            }
            Err(error) => app.notice = Some(format!("{error:#}")),
        }
    }
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let state = &app.replays;
    let analyze = button(text(format!("Analyze {} selected", state.selected.len())))
        .on_press_maybe(
            (!state.selected.is_empty()).then_some(app::Message::Replays(Message::Analyze)),
        );
    let toolbar = row![
        pick_list(Period::ALL, Some(state.period), |period| {
            app::Message::Replays(Message::Period(period))
        }),
        text_input("Search maps", &state.search)
            .on_input(|search| app::Message::Replays(Message::Search(search)))
            .width(260),
        button("Add files…").on_press(app::Message::Replays(Message::AddFiles)),
        scan_status(app),
        space::horizontal(),
        analyze,
    ]
    .spacing(8)
    .align_y(Center);

    let all_selected = state.shown().next().is_some()
        && state.shown().all(|row| state.selected.contains(&row.path));
    let header = row![
        checkbox(all_selected).on_toggle(|on| app::Message::Replays(Message::ToggleAll(on))),
        text("Played").width(140),
        text("Player").width(140),
        text("Map").width(Fill),
        text("Mods").width(80),
        text("Accuracy").width(80),
    ]
    .spacing(12);
    let rows = column(
        state
            .shown()
            .map(|row| row_view(row, state.selected.contains(&row.path))),
    )
    .spacing(4);
    let total = state.added.len() + state.rows.len();
    let footer = if total > MAX_ROWS {
        format!("Showing {MAX_ROWS} of {total} replays")
    } else {
        format!("{total} replays · drop .osr files anywhere to add them")
    };

    column![
        toolbar,
        header,
        scrollable(rows).height(Fill),
        text(footer).size(14)
    ]
    .spacing(12)
    .into()
}

fn row_view(row: &Row, selected: bool) -> Element<'_, app::Message> {
    let path = row.path.clone();
    iced::widget::row![
        checkbox(selected)
            .on_toggle(move |on| app::Message::Replays(Message::Toggle(path.clone(), on))),
        text(played_at(row.played_at_ms)).width(140),
        text(&row.player).width(140),
        text(&row.title).width(Fill),
        text(&row.mods).width(80),
        text(format!("{:.2}%", row.accuracy * 100.0)).width(80),
    ]
    .spacing(12)
    .into()
}

fn scan_status(app: &App) -> Element<'_, app::Message> {
    match app.scanning {
        Some((_, 0)) => text("Scanning osu! folders…").into(),
        Some((done, total)) => row![
            progress_bar(0.0..=total as f32, done as f32).length(160),
            text(format!("{done} / {total} files"))
        ]
        .spacing(8)
        .align_y(Center)
        .into(),
        None => button("Rescan").on_press(app::Message::Rescan).into(),
    }
}

fn played_at(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn select(selected: &mut HashSet<PathBuf>, path: PathBuf, on: bool) {
    if on {
        selected.insert(path);
    } else {
        selected.remove(&path);
    }
}
```

- [ ] **Step 5: Setup page.** `src/ui/setup.rs`:

```rust
use std::fmt;
use std::path::PathBuf;

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input};
use iced::{Center, Element, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::{library, tablets};

use crate::app::{self, App};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Number {
    MinJump,
    MaxError,
    SizeStep,
    RotationStep,
    TabletWidth,
    TabletHeight,
}

impl Number {
    const ANALYSIS: [Number; 4] = [
        Number::MinJump,
        Number::MaxError,
        Number::SizeStep,
        Number::RotationStep,
    ];

    fn label(self) -> &'static str {
        match self {
            Number::MinJump => "Shortest jump analysed (circle radii)",
            Number::MaxError => "Largest miss still analysed (circle radii)",
            Number::SizeStep => "Largest size change per step (%)",
            Number::RotationStep => "Largest rotation change per step (°)",
            Number::TabletWidth => "Width (mm)",
            Number::TabletHeight => "Height (mm)",
        }
    }

    fn get(self, config: &Config) -> Option<f64> {
        match self {
            Number::MinJump => Some(config.fit.min_jump_radii),
            Number::MaxError => Some(config.fit.max_error_radii),
            Number::SizeStep => Some(config.fit.max_size_step * 100.0),
            Number::RotationStep => Some(config.fit.max_rotation_step_deg),
            Number::TabletWidth => config.tablet_size.map(|size| size.0),
            Number::TabletHeight => config.tablet_size.map(|size| size.1),
        }
    }

    // `tablet` is the size in use, so editing one side keeps the other.
    fn set(self, config: &mut Config, value: f64, tablet: (f64, f64)) {
        match self {
            Number::MinJump => config.fit.min_jump_radii = value,
            Number::MaxError => config.fit.max_error_radii = value,
            Number::SizeStep => config.fit.max_size_step = value / 100.0,
            Number::RotationStep => config.fit.max_rotation_step_deg = value,
            Number::TabletWidth => config.tablet_size = Some((value, tablet.1)),
            Number::TabletHeight => config.tablet_size = Some((tablet.0, value)),
        }
    }
}

// Text as typed, so half-entered numbers are not overwritten while editing.
#[derive(Debug)]
pub struct State {
    players: String,
    numbers: Vec<(Number, String)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    AddOsuFolder,
    OsuFolderPicked(Option<PathBuf>),
    DetectOsuFolders,
    ChooseOtdFolder,
    OtdFolderPicked(Option<PathBuf>),
    Profile(Profile),
    Players(String),
    DetectPlayer,
    Number(Number, String),
    UseTableSize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    index: usize,
    label: String,
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl State {
    pub fn new(config: &Config) -> State {
        let numbers = Number::ANALYSIS
            .into_iter()
            .chain([Number::TabletWidth, Number::TabletHeight])
            .map(|number| {
                let text = number.get(config).map(format_number).unwrap_or_default();
                (number, text)
            })
            .collect();
        State {
            players: config.players.join(", "),
            numbers,
        }
    }

    fn text(&self, number: Number) -> &str {
        self.numbers
            .iter()
            .find(|(n, _)| *n == number)
            .map_or("", |(_, text)| text)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    match message {
        Message::AddOsuFolder => return pick_folder(Message::OsuFolderPicked),
        Message::OsuFolderPicked(Some(folder)) => {
            let mut roots = app.config.osu_roots();
            roots.extend(library::roots_for(&folder));
            roots.dedup();
            app.config.osu_roots = Some(roots);
            return rescan(app);
        }
        Message::DetectOsuFolders => {
            app.config.osu_roots = None;
            return rescan(app);
        }
        Message::ChooseOtdFolder => return pick_folder(Message::OtdFolderPicked),
        Message::OtdFolderPicked(Some(folder)) => {
            app.config.otd_dir = Some(folder);
            app.config.profile = 0;
            app.reload_otd();
        }
        Message::OsuFolderPicked(None) | Message::OtdFolderPicked(None) => return Task::none(),
        Message::Profile(profile) => app.config.profile = profile.index,
        Message::Players(players) => {
            app.config.players = players
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            app.setup.players = players;
            app.replays.refresh(&app.library, &app.config);
        }
        Message::DetectPlayer => {
            if let Some(player) = app.library.most_frequent_player() {
                app.config.players = vec![player.to_string()];
                app.setup.players = player.to_string();
                app.replays.refresh(&app.library, &app.config);
            }
        }
        Message::Number(number, value) => {
            if let Ok(parsed) = value.trim().parse::<f64>()
                && parsed.is_finite()
                && parsed > 0.0
            {
                let tablet = tablet_size(app).unwrap_or((parsed, parsed));
                number.set(&mut app.config, parsed, tablet);
            }
            if let Some(entry) = app.setup.numbers.iter_mut().find(|(n, _)| *n == number) {
                entry.1 = value;
            }
        }
        Message::UseTableSize => {
            app.config.tablet_size = None;
            app.setup = State::new(&app.config);
        }
    }
    app.save_config();
    Task::none()
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let osu_roots = column(app.config.osu_roots().into_iter().map(|root| {
        let status = if root.is_dir() { "" } else { "  (not found)" };
        text(format!("{}{status}", root.display())).into()
    }))
    .spacing(4);
    let osu = column![
        heading("osu! folders"),
        osu_roots,
        row![
            button("Add folder…").on_press(app::Message::Setup(Message::AddOsuFolder)),
            button("Detect").on_press(app::Message::Setup(Message::DetectOsuFolders)),
            button("Rescan").on_press_maybe(app.scanning.is_none().then_some(app::Message::Rescan)),
        ]
        .spacing(8),
    ]
    .spacing(8);

    let players = column![
        heading("Players"),
        text("Only these players' replays are listed (comma-separated; empty lists everyone).")
            .size(14),
        row![
            text_input("player names", &app.setup.players)
                .on_input(|players| app::Message::Setup(Message::Players(players)))
                .width(320),
            button("Detect").on_press(app::Message::Setup(Message::DetectPlayer)),
        ]
        .spacing(8)
        .align_y(Center),
    ]
    .spacing(8);

    let analysis = column![heading("Analysis")]
        .extend(Number::ANALYSIS.map(|number| number_input(app, number)))
        .spacing(8);

    scrollable(
        column![
            osu,
            otd_section(app),
            tablet_section(app),
            players,
            analysis
        ]
        .spacing(24)
        .padding([0, 16]),
    )
    .into()
}

fn otd_section(app: &App) -> Element<'_, app::Message> {
    let folder = match app.config.otd_dir() {
        Some(dir) => text(dir.display().to_string()),
        None => text("Not found"),
    };
    let mut section = column![
        heading("OpenTabletDriver"),
        row![
            folder,
            button("Choose folder…").on_press(app::Message::Setup(Message::ChooseOtdFolder)),
        ]
        .spacing(8)
        .align_y(Center),
    ]
    .spacing(8);
    match &app.otd {
        Err(error) => section = section.push(text(error.clone())),
        Ok(settings) => {
            if !settings.is_supported() {
                section = section.push(text(format!(
                    "OpenTabletDriver {} is untested; this app reads the 0.6 settings format.",
                    settings.revision
                )));
            }
            let profiles: Vec<Profile> = settings
                .profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| Profile {
                    index,
                    label: profile.tablet.clone(),
                })
                .collect();
            let selected = profiles.get(app.config.profile).cloned();
            section = section.push(pick_list(profiles, selected, |profile| {
                app::Message::Setup(Message::Profile(profile))
            }));
            if settings
                .profiles
                .get(app.config.profile)
                .is_some_and(|profile| !profile.absolute)
            {
                section = section.push(text(
                    "This profile is not in absolute mode; area suggestions need absolute mode.",
                ));
            }
        }
    }
    section.into()
}

fn tablet_section(app: &App) -> Element<'_, app::Message> {
    let tablet = app
        .otd
        .as_ref()
        .ok()
        .and_then(|settings| settings.profiles.get(app.config.profile))
        .map(|profile| profile.tablet.as_str());
    let line = match (tablet, app.config.tablet_size, tablet.and_then(tablets::tablet_size)) {
        (_, Some(_), _) => format!("{} — size entered by you.", tablet.unwrap_or("Tablet")),
        (Some(name), None, Some((width, height))) => {
            format!("{name} — {width} × {height} mm, from OpenTabletDriver's tablet list.")
        }
        (Some(name), None, None) => {
            format!("{name} — not in OpenTabletDriver's tablet list; enter its active area.")
        }
        (None, None, _) => "No OpenTabletDriver profile yet; choose its folder above or enter                             the tablet's active area."
            .to_string(),
    };
    column![
        heading("Tablet"),
        text(line),
        number_input(app, Number::TabletWidth),
        number_input(app, Number::TabletHeight),
        button("Use OpenTabletDriver's size").on_press_maybe(
            app.config
                .tablet_size
                .is_some()
                .then_some(app::Message::Setup(Message::UseTableSize))
        ),
    ]
    .spacing(8)
    .into()
}

fn number_input(app: &App, number: Number) -> Element<'_, app::Message> {
    let size = tablet_size(app);
    let placeholder = match number {
        Number::TabletWidth => size.map(|size| format_number(size.0)),
        Number::TabletHeight => size.map(|size| format_number(size.1)),
        _ => None,
    }
    .unwrap_or_default();
    row![
        text(number.label()).width(320),
        text_input(&placeholder, app.setup.text(number))
            .on_input(move |value| app::Message::Setup(Message::Number(number, value)))
            .width(120),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn heading(label: &str) -> Element<'_, app::Message> {
    text(label).size(20).into()
}

// The size in use: the user's own entry first, then the bundled table.
pub fn tablet_size(app: &App) -> Option<(f64, f64)> {
    app.config.tablet_size.or_else(|| {
        let settings = app.otd.as_ref().ok()?;
        tablets::tablet_size(&settings.profiles.get(app.config.profile)?.tablet)
    })
}

fn pick_folder(picked: fn(Option<PathBuf>) -> Message) -> Task<app::Message> {
    Task::perform(rfd::AsyncFileDialog::new().pick_folder(), move |folder| {
        app::Message::Setup(picked(folder.map(|folder| folder.path().to_path_buf())))
    })
}

fn rescan(app: &mut App) -> Task<app::Message> {
    app.save_config();
    Task::done(app::Message::Rescan)
}

fn format_number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    rounded.to_string()
}
```

- [ ] **Step 6: Gate.** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`. Expected: clean, with all core tests passing.
- [ ] **Step 7: Run it.** Start `cargo run`. On first run (no `%APPDATA%/osu-tablet-optimizer/config.json`) it opens on Setup, and the scan fills the cache and auto-selects the player. With a config it opens on Replays.
  - **Screenshots:** capture only the app window, with `PrintWindow` in a job with a timeout. Never copy screen areas, because that can capture other windows.
  - **Expected Replays page:** the period picker, search, "Add files…", the scan status ("Scanning…", then a progress bar, then "Rescan") and a list of rows.
  - **Expected Setup page:** detected roots, the OTD folder (or "Not found" with "Choose folder…"), the profile picker, the tablet line, players and the analysis limits.
- [ ] **Step 8: Commit** `feat: Setup and Replays pages`.

---

### Task 5: Docs, verify on GitHub, merge

- [ ] **README:** add a `## Usage` section covering first start (Setup), choosing a portable OpenTabletDriver folder, players, Replays filters, Add files and drag-and-drop, and where settings and cache live.
- [ ] **CLAUDE.md:** under Architecture, mention `src/app.rs` (state, background scan) and that `src/ui/<page>.rs` has `view` and `update` over `&mut App`.
- [ ] **Gate:** `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`.
- [ ] **PR:** push, open the PR to `dev`, check CI (a Linux build with `rfd` xdg-portal), get a whole-branch review, then squash-merge and clean up.
- [ ] **Continue with feature 8.** Don't ask about dev→main until feature 9.

## Notes for later features

- Feature 8: the Analysis page reads `app.replays.selected`. Run `hits::replay_samples` and `fit::suggest` off the UI thread, the same way as `App::scan`.
- Feature 9: the Area page writes presets with `OtdSettings::write_preset` and uses `setup::tablet_size(app)`.
