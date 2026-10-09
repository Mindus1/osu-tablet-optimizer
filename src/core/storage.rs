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
