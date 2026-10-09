# Beatmap Index Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `core::library` finds the user's osu! installs, indexes every beatmap (by MD5, with title) and every replay (header) in stable and lazer, caches the result, and rescans incrementally.

**Architecture:** One module, `src/core/library.rs`. It's named for what it holds (beatmaps *and* replays), so the spec's `beatmap.rs` becomes `library.rs`. Scanning walks the roots and keeps only *candidates*: files ending in `.osu` or `.osr`, or with no extension at all (lazer's hashed file store). It reuses every cache entry whose modified time and size are unchanged, and reads the rest in parallel. Files are classified by content (`osu file format v` signature, or the `.osr` header signature), so one code path serves stable and lazer. The cache is one JSON file.

**Measured on the test machine** (stable 108k files, lazer store 121k): a native walk takes under 1 s; a cold read costs about 7 ms per file on one thread and about 1 ms with 8 threads. A first full scan (171k candidates) took 25 s and a rescan 1.5 s. The cache is 51 MB of JSON and loads in 0.25 s. This replaces the spec's "incremental by directory mtime": a per-file modified time + size check is just as cheap, and it also catches files edited in place.

**Tech Stack:** Rust 2024, `dirs` 7 (platform data dirs), `md5` 0.8 (zero-dependency; `md-5` 0.11's digest can't be hex-formatted), `rosu-map` 0.2 (`Metadata` section for titles), `serde`/`serde_json` (cache), `std::thread::scope` (parallel reads).

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- `src/core/` is pure logic, no iced. Tests live in the module.
- General-purpose: no hardcoded user paths or names. Install locations come from `dirs` plus each client's own config (stable `osu!.<user>.cfg` `BeatmapDirectory`, lazer `storage.ini` `FullPath`).
- `anyhow::Result` + `.context(..)` at I/O boundaries. Never `unwrap()` on user data (tests may unwrap).
- Comments only for a non-obvious *why*. Mark deliberate ceilings with `ponytail:`.
- Tests that need the real installs are `#[ignore]`d and run with `cargo test -- --ignored`.
- Commits: Conventional Commits ending with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR
  ```

## Review Focus

- **Empty extension-less files** (common in lazer's store): expected to be indexed as "other", with no error. Pinned by `empty_extensionless_file_is_other` (Task 2).
- **Uppercase `.OSU` / `.OSR` extensions:** expected to be candidates. Pinned by `uppercase_extensions_are_candidates` (Task 2).
- **A beatmap with no `[Metadata]` section:** expected to be indexed by MD5 anyway (the title may be blank). Pinned by `beatmap_without_metadata_is_still_indexed` (Task 2).
- **The same beatmap in both stable and lazer:** expected to give one MD5 key. Pinned by `duplicate_beatmaps_share_one_key` (Task 2).
- **A corrupt cache file:** expected `load` to return an error, so the caller can rescan from scratch instead of crashing. Pinned by `corrupt_cache_is_an_error` (Task 3).

---

### Task 1: Install locations

**Files:**
- Modify: `Cargo.toml` (dep `dirs`), `src/core.rs`
- Create: `src/core/library.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn default_roots() -> Vec<PathBuf>`, `pub fn stable_roots(stable: &Path) -> Vec<PathBuf>`, `pub fn lazer_root(default: &Path) -> PathBuf`. Private `songs_dir`, `ini_value`. Test helper `TempDir` (`new(name)`, `write(relative, bytes) -> PathBuf`, field `.0`).

- [ ] **Step 1: Add the dependency and register the module**

Run: `cargo add dirs@7`

Replace `src/core.rs` with:

```rust
pub mod library;
pub mod replay;
```

- [ ] **Step 2: Write the failing tests**

Create `src/core/library.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("oto-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn ini_value_reads_trimmed_values() {
        let text = "# comment\r\nFullPath = D:\\osu lazer \r\nEmpty =\r\n";

        assert_eq!(ini_value(text, "FullPath").as_deref(), Some("D:\\osu lazer"));
        assert_eq!(ini_value(text, "Empty"), None);
        assert_eq!(ini_value(text, "Missing"), None);
    }

    #[test]
    fn lazer_root_follows_storage_ini() {
        let dir = TempDir::new("lazer");
        assert_eq!(lazer_root(&dir.0), dir.0);

        dir.write("storage.ini", "FullPath = D:\\Games\\osu-data\n");

        assert_eq!(lazer_root(&dir.0), PathBuf::from("D:\\Games\\osu-data"));
    }

    #[test]
    fn stable_roots_add_songs_folder_outside_install() {
        let dir = TempDir::new("stable");
        assert_eq!(stable_roots(&dir.0), [dir.0.clone()]);

        let elsewhere = std::env::temp_dir().join("oto-elsewhere-songs");
        dir.write("osu!.cfg", "BeatmapDirectory = Ignored\n");
        dir.write(
            "osu!.player.cfg",
            format!("BeatmapDirectory = {}\n", elsewhere.display()),
        );

        assert_eq!(stable_roots(&dir.0), [dir.0.clone(), elsewhere]);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::library`
Expected: compile errors `cannot find function ini_value / lazer_root / stable_roots`, `cannot find type PathBuf`.

- [ ] **Step 4: Implement (above the test module)**

```rust
use std::fs;
use std::path::{Path, PathBuf};

pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(stable) = dirs::data_local_dir()
        .map(|dir| dir.join("osu!"))
        .filter(|dir| dir.is_dir())
    {
        roots.extend(stable_roots(&stable));
    }
    if let Some(lazer) = dirs::data_dir()
        .map(|dir| dir.join("osu"))
        .filter(|dir| dir.is_dir())
    {
        roots.push(lazer_root(&lazer));
    }
    roots
}

// Stable's Songs folder can be moved anywhere via BeatmapDirectory in the user's osu!.<name>.cfg.
pub fn stable_roots(stable: &Path) -> Vec<PathBuf> {
    let songs = songs_dir(stable);
    if songs.starts_with(stable) {
        vec![stable.to_path_buf()]
    } else {
        vec![stable.to_path_buf(), songs]
    }
}

// lazer can move its data; the default folder then only keeps a storage.ini pointing there.
pub fn lazer_root(default: &Path) -> PathBuf {
    fs::read_to_string(default.join("storage.ini"))
        .ok()
        .and_then(|text| ini_value(&text, "FullPath"))
        .map_or_else(|| default.to_path_buf(), PathBuf::from)
}

fn songs_dir(stable: &Path) -> PathBuf {
    let newest_user_cfg = fs::read_dir(stable)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("osu!.") && name.ends_with(".cfg") && name != "osu!.cfg"
        })
        .max_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    let configured = newest_user_cfg
        .and_then(|entry| fs::read_to_string(entry.path()).ok())
        .and_then(|text| ini_value(&text, "BeatmapDirectory"));
    stable.join(configured.unwrap_or_else(|| "Songs".to_string()))
}

fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::library`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add Cargo.toml Cargo.lock src/core.rs src/core/library.rs
git commit -m "feat: locate stable and lazer installs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 2: Scanning and classification

**Files:**
- Modify: `Cargo.toml` (deps `md5`, `rosu-map`), `src/core/library.rs`, `src/core/replay.rs` (share the test `Osr` builder)

**Interfaces:**
- Consumes: Task 1's `TempDir`. Also `replay::read_header`, `replay::Header`, and `replay::tests::Osr` (made `pub(crate)` here).
- Produces: `pub struct Library` (`Default`; private `files: HashMap<PathBuf, Entry>`).
  `pub fn scan(&mut self, roots: &[PathBuf]) -> Vec<PathBuf>` returns the files it couldn't read; they aren't cached, so the next scan retries them.
  `pub fn beatmaps_by_md5(&self) -> HashMap<&str, Beatmap<'_>>`, with `pub struct Beatmap<'a> { pub path: &'a Path, pub title: &'a str }` (title = `"Artist - Title [Version]"`).
  `pub fn replays(&self) -> impl Iterator<Item = (&Path, &Header)>`.
  Private: `enum Content { Beatmap { md5, title }, Replay(Header), Other }`, `struct Entry { modified: SystemTime, len: u64, content: Content }`, plus `collect_candidates`, `is_candidate`, `read_content`, `is_beatmap`, `is_replay`, `beatmap_content`, `par_map`. Test helpers `md5_hex`, `replay_bytes`, `scanned`.

- [ ] **Step 1: Add dependencies and share the test replay builder**

Run: `cargo add md5@0.8 rosu-map@0.2`

In `src/core/replay.rs`, change `mod tests {` to `pub(crate) mod tests {`. Then change `struct Osr<'a> {` to `pub(crate) struct Osr<'a> {`, prefix each of its six fields with `pub(crate) `, and change `fn bytes(&self) -> Vec<u8> {` to `pub(crate) fn bytes(&self) -> Vec<u8> {`.

- [ ] **Step 2: Write the failing tests**

In `src/core/library.rs`, replace `use super::*;` inside `mod tests` with:

```rust
    use std::fs::FileTimes;
    use std::time::Duration;

    use super::*;
    use crate::core::replay::tests::Osr;

    const BEATMAP: &str =
        "osu file format v14\n\n[Metadata]\nTitle:Song\nArtist:Band\nVersion:Hard\n";
```

Add after `impl Drop for TempDir { .. }`:

```rust
    fn md5_hex(bytes: &[u8]) -> String {
        format!("{:x}", md5::compute(bytes))
    }

    fn replay_bytes(player: &str) -> Vec<u8> {
        Osr {
            player,
            ..Osr::default()
        }
        .bytes()
    }

    fn scanned(dir: &TempDir) -> Library {
        let mut library = Library::default();
        assert!(library.scan(&[dir.0.clone()]).is_empty());
        library
    }

    fn set_modified(path: &Path, modified: SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();
    }
```

Add at the end of `mod tests`:

```rust
    #[test]
    fn indexes_osu_files_by_md5_with_title() {
        let dir = TempDir::new("osu-files");
        let path = dir.write("Songs/1 Band - Song/map.osu", BEATMAP);

        let library = scanned(&dir);

        let beatmaps = library.beatmaps_by_md5();
        let beatmap = beatmaps[md5_hex(BEATMAP.as_bytes()).as_str()];
        assert_eq!(beatmap.path, path);
        assert_eq!(beatmap.title, "Band - Song [Hard]");
    }

    #[test]
    fn sniffs_extensionless_beatmaps_and_replays() {
        let dir = TempDir::new("sniff");
        let with_bom = format!("\u{feff}{BEATMAP}");
        dir.write("files/a/ab/abc", &with_bom);
        dir.write("files/c/cd/cde", replay_bytes("player"));
        dir.write("files/e/ef/efg", b"ID3\x04\x00 not osu data");

        let library = scanned(&dir);

        assert!(
            library
                .beatmaps_by_md5()
                .contains_key(md5_hex(with_bom.as_bytes()).as_str())
        );
        assert_eq!(library.replays().count(), 1);
        assert_eq!(library.files.len(), 3);
    }

    #[test]
    fn other_extensions_are_never_read() {
        let dir = TempDir::new("extensions");
        dir.write("Songs/set/bg.jpg", BEATMAP);
        dir.write("Data/r/replay.osg", replay_bytes("player"));

        assert!(scanned(&dir).files.is_empty());
    }

    #[test]
    fn uppercase_extensions_are_candidates() {
        let dir = TempDir::new("uppercase");
        dir.write("Songs/set/MAP.OSU", BEATMAP);
        dir.write("Replays/R.OSR", replay_bytes("player"));

        let library = scanned(&dir);

        assert_eq!(library.beatmaps_by_md5().len(), 1);
        assert_eq!(library.replays().count(), 1);
    }

    #[test]
    fn empty_extensionless_file_is_other() {
        let dir = TempDir::new("empty");
        dir.write("files/0/00/empty", b"");

        let library = scanned(&dir);

        assert_eq!(library.files.len(), 1);
        assert!(library.beatmaps_by_md5().is_empty());
    }

    #[test]
    fn beatmap_without_metadata_is_still_indexed() {
        let dir = TempDir::new("no-metadata");
        dir.write("Songs/set/map.osu", "osu file format v3\n");

        let library = scanned(&dir);

        assert!(
            library
                .beatmaps_by_md5()
                .contains_key(md5_hex(b"osu file format v3\n").as_str())
        );
    }

    #[test]
    fn duplicate_beatmaps_share_one_key() {
        let dir = TempDir::new("duplicates");
        dir.write("stable/Songs/set/map.osu", BEATMAP);
        dir.write("lazer/files/a/ab/abcdef", BEATMAP);

        assert_eq!(scanned(&dir).beatmaps_by_md5().len(), 1);
    }

    #[test]
    fn indexes_replay_headers() {
        let dir = TempDir::new("replays");
        let path = dir.write("Data/r/abc-123.osr", replay_bytes("someone"));

        let library = scanned(&dir);

        let replays: Vec<_> = library.replays().collect();
        assert_eq!(replays.len(), 1);
        assert_eq!(replays[0].0, path);
        assert_eq!(replays[0].1.player, "someone");
    }

    #[test]
    fn corrupt_replay_counts_as_other() {
        let dir = TempDir::new("corrupt-replay");
        let bytes = replay_bytes("player");
        dir.write("Data/r/cut.osr", &bytes[..20]);

        let library = scanned(&dir);

        assert_eq!(library.replays().count(), 0);
        assert_eq!(library.files.len(), 1);
    }

    #[test]
    fn unchanged_files_are_not_reread() {
        let dir = TempDir::new("unchanged");
        let path = dir.write("Songs/set/map.osu", BEATMAP);
        let mut library = scanned(&dir);
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, BEATMAP.replace("Hard", "Easy")).unwrap();
        set_modified(&path, modified);

        library.scan(&[dir.0.clone()]);

        assert!(
            library
                .beatmaps_by_md5()
                .contains_key(md5_hex(BEATMAP.as_bytes()).as_str())
        );
    }

    #[test]
    fn changed_files_are_reread() {
        let dir = TempDir::new("changed");
        let path = dir.write("Songs/set/map.osu", BEATMAP);
        let mut library = scanned(&dir);
        let edited = format!("{BEATMAP}Creator:someone\n");
        fs::write(&path, &edited).unwrap();
        set_modified(&path, SystemTime::now() + Duration::from_secs(5));

        library.scan(&[dir.0.clone()]);

        let beatmaps = library.beatmaps_by_md5();
        assert!(beatmaps.contains_key(md5_hex(edited.as_bytes()).as_str()));
        assert_eq!(beatmaps.len(), 1);
    }

    #[test]
    fn deleted_files_are_dropped() {
        let dir = TempDir::new("deleted");
        let path = dir.write("Songs/set/map.osu", BEATMAP);
        let mut library = scanned(&dir);
        fs::remove_file(&path).unwrap();

        library.scan(&[dir.0.clone()]);

        assert!(library.files.is_empty());
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::library`
Expected: compile errors `cannot find type Library`, `cannot find type SystemTime`, `cannot find type File`.

- [ ] **Step 4: Implement**

Replace the `use` block at the top of `src/core/library.rs` with:

```rust
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::SystemTime;

use anyhow::Result;
use rosu_map::section::metadata::Metadata;

use crate::core::replay::{self, Header};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const BEATMAP_SIGNATURE: &[u8] = b"osu file format v";

#[derive(Debug, Clone, PartialEq)]
enum Content {
    Beatmap { md5: String, title: String },
    Replay(Header),
    Other,
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    modified: SystemTime,
    len: u64,
    content: Content,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beatmap<'a> {
    pub path: &'a Path,
    pub title: &'a str,
}

#[derive(Debug, Default)]
pub struct Library {
    files: HashMap<PathBuf, Entry>,
}

impl Library {
    // Returns the files that could not be read; the next scan retries them.
    pub fn scan(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        for root in roots {
            collect_candidates(root, &mut candidates);
        }

        let mut previous = std::mem::take(&mut self.files);
        let mut changed = Vec::new();
        for (path, modified, len) in candidates {
            match previous.remove(&path) {
                Some(entry) if entry.modified == modified && entry.len == len => {
                    self.files.insert(path, entry);
                }
                _ => changed.push((path, modified, len)),
            }
        }

        let contents = par_map(&changed, |(path, _, _)| read_content(path));
        let mut unreadable = Vec::new();
        for ((path, modified, len), content) in changed.into_iter().zip(contents) {
            match content {
                Ok(content) => {
                    self.files.insert(
                        path,
                        Entry {
                            modified,
                            len,
                            content,
                        },
                    );
                }
                Err(_) => unreadable.push(path),
            }
        }
        unreadable
    }

    pub fn beatmaps_by_md5(&self) -> HashMap<&str, Beatmap<'_>> {
        self.files
            .iter()
            .filter_map(|(path, entry)| match &entry.content {
                Content::Beatmap { md5, title } => Some((md5.as_str(), Beatmap { path, title })),
                _ => None,
            })
            .collect()
    }

    pub fn replays(&self) -> impl Iterator<Item = (&Path, &Header)> {
        self.files
            .iter()
            .filter_map(|(path, entry)| match &entry.content {
                Content::Replay(header) => Some((path.as_path(), header)),
                _ => None,
            })
    }
}
```

(Keep `default_roots`, `stable_roots`, `lazer_root`, `songs_dir` and `ini_value` from Task 1 below this.)

Add after `fn ini_value`:

```rust
fn collect_candidates(dir: &Path, out: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_candidates(&path, out);
        } else if meta.is_file() && is_candidate(&path) {
            if let Ok(modified) = meta.modified() {
                out.push((path, modified, meta.len()));
            }
        }
    }
}

// lazer's file store has no extensions, so extension-less files are sniffed too.
fn is_candidate(path: &Path) -> bool {
    match path.extension() {
        None => true,
        Some(ext) => ext.eq_ignore_ascii_case("osu") || ext.eq_ignore_ascii_case("osr"),
    }
}

fn read_content(path: &Path) -> Result<Content> {
    let mut file = BufReader::new(File::open(path)?);
    let prefix = file.fill_buf()?;
    let (beatmap, replay) = (is_beatmap(prefix), is_replay(prefix));
    if beatmap {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        return Ok(beatmap_content(&bytes));
    }
    if replay {
        return Ok(replay::read_header(&mut file).map_or(Content::Other, Content::Replay));
    }
    Ok(Content::Other)
}

fn is_beatmap(prefix: &[u8]) -> bool {
    prefix
        .strip_prefix(UTF8_BOM)
        .unwrap_or(prefix)
        .trim_ascii_start()
        .starts_with(BEATMAP_SIGNATURE)
}

// Mode byte, a plausible i32 version, then the beatmap MD5 as a present 32-byte string.
fn is_replay(prefix: &[u8]) -> bool {
    match prefix {
        [mode, v0, v1, v2, v3, 0x0b, 32, ..] if *mode <= 3 => {
            (20_000_000..40_000_000).contains(&i32::from_le_bytes([*v0, *v1, *v2, *v3]))
        }
        _ => false,
    }
}

fn beatmap_content(bytes: &[u8]) -> Content {
    let title = rosu_map::from_bytes::<Metadata>(bytes)
        .map(|meta| format!("{} - {} [{}]", meta.artist, meta.title, meta.version))
        .unwrap_or_default();
    Content::Beatmap {
        md5: format!("{:x}", md5::compute(bytes)),
        title,
    }
}

// Cold reads dominate a first scan; reading in parallel made it about 7x faster in measurements.
// ponytail: equal static chunks per thread; switch to work stealing if one chunk dominates.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    let chunk_len = items.len().div_ceil(threads).max(1);
    thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(chunk_len)
            .map(|chunk| {
                let f = &f;
                scope.spawn(move || chunk.iter().map(f).collect::<Vec<_>>())
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("library scan worker panicked"))
            .collect()
    })
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::`
Expected: `core::library` 15 passed, and the `core::replay` tests still pass (23).

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add Cargo.toml Cargo.lock src/core/library.rs src/core/replay.rs
git commit -m "feat: index beatmaps and replays in stable and lazer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 3: Cache persistence and player detection

**Files:**
- Modify: `src/core/library.rs`, `src/core/replay.rs` (serde derives on `Header`)

**Interfaces:**
- Consumes: Task 2's `Library`, `Content`, `Entry`, test helpers.
- Produces: `pub fn load(path: &Path) -> anyhow::Result<Library>` (missing file gives an empty library; an unreadable or corrupt file gives `Err`). `pub fn save(&self, path: &Path) -> anyhow::Result<()>` (creates parent dirs). `pub fn most_frequent_player(&self) -> Option<&str>` (ignores empty names; ties go to the alphabetically first name). `replay::Header` gets `Serialize, Deserialize`.

- [ ] **Step 1: Write the failing tests**

Add at the end of `mod tests` in `src/core/library.rs`:

```rust
    #[test]
    fn cache_survives_save_and_load() {
        let dir = TempDir::new("cache");
        dir.write("Songs/set/map.osu", BEATMAP);
        dir.write("Data/r/x.osr", replay_bytes("player"));
        let library = scanned(&dir);
        let cache = dir.0.join("cache/library.json");

        library.save(&cache).unwrap();
        let loaded = Library::load(&cache).unwrap();

        assert_eq!(loaded.files, library.files);
    }

    #[test]
    fn missing_cache_loads_empty() {
        let dir = TempDir::new("no-cache");

        assert!(
            Library::load(&dir.0.join("library.json"))
                .unwrap()
                .files
                .is_empty()
        );
    }

    #[test]
    fn corrupt_cache_is_an_error() {
        let dir = TempDir::new("corrupt-cache");
        let cache = dir.write("library.json", "{not json");

        assert!(Library::load(&cache).is_err());
    }

    #[test]
    fn most_frequent_player_wins() {
        let dir = TempDir::new("players");
        dir.write("r/1.osr", replay_bytes("alice"));
        dir.write("r/2.osr", replay_bytes("bob"));
        dir.write("r/3.osr", replay_bytes("bob"));
        dir.write("r/4.osr", replay_bytes(""));
        dir.write("r/5.osr", replay_bytes(""));
        dir.write("r/6.osr", replay_bytes(""));

        assert_eq!(scanned(&dir).most_frequent_player(), Some("bob"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::library`
Expected: compile errors `no function or associated item named load / save`, `no method named most_frequent_player`.

- [ ] **Step 3: Implement**

In `src/core/replay.rs`, change `use serde::Deserialize;` to `use serde::{Deserialize, Serialize};`, and change the derive above `pub struct Header` to:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
```

In `src/core/library.rs`, replace the `use std::io::...` and `use anyhow::...` lines and add `serde`:

```rust
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
```

```rust
use anyhow::{Context, Result};
use rosu_map::section::metadata::Metadata;
use serde::{Deserialize, Serialize};
```

Add `Serialize, Deserialize` to the derives of `Content` and `Entry` (`#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`) and of `Library` (`#[derive(Debug, Default, Serialize, Deserialize)]`).

Add at the start of `impl Library`:

```rust
    pub fn load(path: &Path) -> Result<Library> {
        if !path.exists() {
            return Ok(Library::default());
        }
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("reading {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, self)?;
        writer.flush()?;
        Ok(())
    }
```

Add at the end of `impl Library`:

```rust
    pub fn most_frequent_player(&self) -> Option<&str> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for (_, header) in self.replays() {
            if !header.player.is_empty() {
                *counts.entry(&header.player).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|&(name, count)| (count, std::cmp::Reverse(name)))
            .map(|(name, _)| name)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib core::`
Expected: `core::library` 19 passed, `core::replay` 23 passed.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add src/core/library.rs src/core/replay.rs
git commit -m "feat: persist library cache and detect the player

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 4: Real-install check and docs

**Files:**
- Modify: `src/core/library.rs` (ignored test), `CLAUDE.md` (Commands), `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

**Interfaces:**
- Consumes: everything above.
- Produces: `real_installs_resolve_fixture_beatmaps` (`#[ignore]`).

- [ ] **Step 1: Add the ignored test at the end of `mod tests`**

```rust
    #[test]
    #[ignore = "scans the osu! installs on this machine"]
    fn real_installs_resolve_fixture_beatmaps() {
        let roots = default_roots();
        let mut library = Library::default();
        let started = std::time::Instant::now();
        let unreadable = library.scan(&roots);
        let first = started.elapsed();
        let started = std::time::Instant::now();
        library.scan(&roots);
        eprintln!(
            "{roots:?}: {} files, first scan {first:?}, rescan {:?}, {} unreadable, player {:?}",
            library.files.len(),
            started.elapsed(),
            unreadable.len(),
            library.most_frequent_player()
        );

        let beatmaps = library.beatmaps_by_md5();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for path in fs::read_dir(fixtures).unwrap().map(|e| e.unwrap().path()) {
            if path.extension().is_some_and(|ext| ext == "osr") {
                let header =
                    replay::read_header(&mut BufReader::new(File::open(&path).unwrap())).unwrap();
                let beatmap = beatmaps.get(header.beatmap_md5.as_str());
                assert!(beatmap.is_some(), "{}: beatmap not found", path.display());
                eprintln!("{} -> {}", path.display(), beatmap.unwrap().title);
            }
        }
    }
```

- [ ] **Step 2: Run it on this machine**

Run: `cargo test --lib core::library -- --ignored --nocapture`
Expected: `1 passed`. The output shows both osu! roots, more than 100k files, a rescan well under the first scan, `player Some(..)`, and every fixture `.osr` mapped to its beatmap title.

- [ ] **Step 3: Docs**

In `CLAUDE.md`, under `## Commands`, add after the `cargo test` line:

```sh
cargo test -- --ignored                     # scans the real osu! installs on this machine
```

In the spec, under `## Architecture`, replace the `src/core/beatmap.rs` line with:

```
src/core/library.rs    find stable/lazer installs, index beatmaps (md5, path, title) + replay headers, JSON cache
```

In the spec, in the **Data discovery** list, replace the sentence that starts "One scan pass over the lazer store sorts files by signature" (through the end of that bullet) with:

```markdown
- One scan walks every root and keeps `.osu`, `.osr` and extension-less files (lazer's store). It reuses cache entries whose modified time and size are unchanged, and classifies the rest by content: `osu file format v`, or a `.osr` header signature (mode byte 0–3, plausible version, `0x0b 0x20` + 32-char MD5). Changed files are read in parallel. On the test machine: 171k candidates, 25 s first scan, 1.5 s rescan.
```

and replace the persistence bullet's `index.tsv (...)` with `library.json` (file cache: path → modified, size, beatmap md5 + title or replay header).

- [ ] **Step 4: Commit**

```bash
git add src/core/library.rs CLAUDE.md docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md
git commit -m "test: resolve fixture beatmaps against the real installs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 5: Verify on GitHub and merge (CLAUDE.md feature flow)

**Files:** none new.

**Interfaces:**
- Consumes: Tasks 1–4 on `feature/beatmap-index`.
- Produces: `core::library` merged into `dev`.

- [ ] **Step 1: Local gate**

Run: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`
Expected: all pass (42 tests, 1 ignored).

- [ ] **Step 2: Refactor new code only**, re-run Step 1, and commit `refactor: ...` if anything changed.

- [ ] **Step 3: README check**: still nothing user-visible; confirm.

- [ ] **Step 4: Push, open the PR, CI green**

```bash
git push -u origin feature/beatmap-index
gh pr create --base dev --title "feat: beatmap and replay library index" --body "core::library: finds stable/lazer installs, indexes beatmaps by MD5 and replay headers, incremental JSON cache.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks --watch
```

- [ ] **Step 5: Whole-branch review, then squash-merge and clean up**

```bash
gh pr merge --squash --delete-branch
git switch dev && git pull && git fetch --prune
git branch -vv | grep ': gone]' || true
```

- [ ] **Step 6: Ask the user** "Merge dev into main?"

## Notes for later features

- Feature 7 (Replays page): run `Library::scan` in a background task. Load the cache from, and save it to, `dirs::data_dir()/osu-tablet-optimizer/library.json`. If `load` returns `Err` (corrupt cache), start from `Library::default()` and rescan. Build `beatmaps_by_md5()` once per render pass, not per row. Scan progress reporting gets added there.
- Feature 4: open beatmaps through `beatmaps_by_md5()[md5].path`, and read whole replays with `BufReader`.
- Feature 7, from the final review (deferred minors):
  - `scan` replaces the whole library with what's under the given roots, so always pass every root. Don't scan a single folder for "Add files…".
  - Check that user-chosen roots exist (`is_dir`) and say so if they don't. A missing root is otherwise silently empty.
  - Make `save` atomic (write `library.json.tmp`, then rename it over the old file).
  - Add a cache `version` field when title or header building changes.
  - Treat a worker panic as unreadable (`catch_unwind`) if `rosu-map` ever panics on a malformed file.
  - Wine: `BeatmapDirectory = D:\…` from a Wine stable install needs mapping to the Wine prefix's `drive_d`.
