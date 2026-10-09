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
}
