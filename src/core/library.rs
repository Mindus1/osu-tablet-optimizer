use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::SystemTime;

use anyhow::{Context, Result};
use rosu_map::section::metadata::Metadata;
use serde::{Deserialize, Serialize};

use crate::core::replay::{self, Header};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const BEATMAP_SIGNATURE: &[u8] = b"osu file format v";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Content {
    Beatmap { md5: String, title: String },
    Replay(Header),
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Library {
    files: HashMap<PathBuf, Entry>,
}

impl Library {
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

    // Returns the files that could not be read or cached; the next scan retries them.
    pub fn scan(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        for root in roots {
            collect_candidates(root, &mut candidates);
        }
        let (candidates, uncacheable): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .partition(|(path, modified, _)| is_cacheable(path, *modified));
        let mut unreadable: Vec<PathBuf> = uncacheable.into_iter().map(|(path, ..)| path).collect();

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
}

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

// Stable's Songs folder can be moved anywhere via BeatmapDirectory in the user's osu!.<name>.cfg,
// or left behind as a junction/symlink, which the walk doesn't follow. Resolving it gives the
// real folder its own root.
pub fn stable_roots(stable: &Path) -> Vec<PathBuf> {
    let songs = songs_dir(stable);
    let resolved = fs::canonicalize(&songs).ok();
    let inside_stable = match (&resolved, fs::canonicalize(stable)) {
        (Some(songs), Ok(stable)) => songs.starts_with(stable),
        _ => songs.starts_with(stable),
    };
    if inside_stable {
        vec![stable.to_path_buf()]
    } else {
        vec![stable.to_path_buf(), resolved.unwrap_or(songs)]
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
        } else if meta.is_file()
            && is_candidate(&path)
            && let Ok(modified) = meta.modified()
        {
            out.push((path, modified, meta.len()));
        }
    }
}

// The JSON cache only holds UTF-8 paths and post-1970 times; anything else would make save() fail.
fn is_cacheable(path: &Path, modified: SystemTime) -> bool {
    path.to_str().is_some() && modified >= SystemTime::UNIX_EPOCH
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

#[cfg(test)]
mod tests {
    use std::fs::FileTimes;
    use std::time::Duration;

    use super::*;
    use crate::core::replay::tests::Osr;

    const BEATMAP: &str =
        "osu file format v14\n\n[Metadata]\nTitle:Song\nArtist:Band\nVersion:Hard\n";

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

        fn roots(&self) -> &[PathBuf] {
            std::slice::from_ref(&self.0)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

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
        assert!(library.scan(dir.roots()).is_empty());
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

    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[cfg(unix)]
    fn link_dir(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn ini_value_reads_trimmed_values() {
        let text = "# comment\r\nFullPath = D:\\osu lazer \r\nEmpty =\r\n";

        assert_eq!(
            ini_value(text, "FullPath").as_deref(),
            Some("D:\\osu lazer")
        );
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
        assert_eq!(stable_roots(&dir.0), [dir.0.as_path()]);

        let elsewhere = std::env::temp_dir().join("oto-elsewhere-songs");
        dir.write("osu!.cfg", "BeatmapDirectory = Ignored\n");
        dir.write(
            "osu!.player.cfg",
            format!("BeatmapDirectory = {}\n", elsewhere.display()),
        );

        assert_eq!(stable_roots(&dir.0), [dir.0.clone(), elsewhere]);
    }

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

        library.scan(dir.roots());

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

        library.scan(dir.roots());

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

        library.scan(dir.roots());

        assert!(library.files.is_empty());
    }

    #[test]
    fn linked_songs_folder_is_scanned() {
        let stable = TempDir::new("linked-stable");
        let songs = TempDir::new("linked-songs");
        songs.write("set/map.osu", BEATMAP);
        link_dir(&songs.0, &stable.0.join("Songs"));

        let mut library = Library::default();
        assert!(library.scan(&stable_roots(&stable.0)).is_empty());

        assert!(
            library
                .beatmaps_by_md5()
                .contains_key(md5_hex(BEATMAP.as_bytes()).as_str())
        );
    }

    #[test]
    fn files_the_cache_cannot_hold_are_reported_not_cached() {
        let dir = TempDir::new("pre-epoch");
        let path = dir.write("Songs/set/map.osu", BEATMAP);
        set_modified(&path, SystemTime::UNIX_EPOCH - Duration::from_secs(60));

        let mut library = Library::default();
        let unreadable = library.scan(dir.roots());

        assert_eq!(unreadable, [path]);
        library.save(&dir.0.join("library.json")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_reported_not_cached() {
        use std::os::unix::ffi::OsStrExt;
        let dir = TempDir::new("non-utf8");
        let path = dir.0.join(std::ffi::OsStr::from_bytes(b"\xffmap.osu"));
        fs::write(&path, BEATMAP).unwrap();

        let mut library = Library::default();
        let unreadable = library.scan(dir.roots());

        assert_eq!(unreadable, [path]);
        library.save(&dir.0.join("library.json")).unwrap();
    }

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
}
