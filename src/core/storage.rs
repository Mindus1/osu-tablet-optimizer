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

// Moves a file that can't be read out of the way, so saving a fresh one later keeps it.
pub fn set_aside(path: &Path) -> Result<PathBuf> {
    let aside = path.with_extension("json.bad");
    fs::rename(path, &aside).with_context(|| format!("moving {} aside", path.display()))?;
    Ok(aside)
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

    #[test]
    fn unreadable_file_is_set_aside() {
        let dir = TempDir::new("storage-aside");
        let path = dir.write("config.json", "{ not json");

        let aside = set_aside(&path).unwrap();

        assert!(!path.exists());
        assert_eq!(fs::read_to_string(aside).unwrap(), "{ not json");
    }
}
