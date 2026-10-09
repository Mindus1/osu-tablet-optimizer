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

// Moves a config that can't be read out of the way, so saving the defaults later keeps it.
pub fn set_aside(path: &Path) -> Result<PathBuf> {
    let aside = path.with_extension("json.bad");
    fs::rename(path, &aside).with_context(|| format!("moving {} aside", path.display()))?;
    Ok(aside)
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
    fn unreadable_config_is_set_aside() {
        let dir = TempDir::new("config-aside");
        let path = dir.write("config.json", "{ not json");

        let aside = set_aside(&path).unwrap();

        assert!(!path.exists());
        assert_eq!(fs::read_to_string(aside).unwrap(), "{ not json");
    }

    #[test]
    fn corrupt_config_is_an_error() {
        let dir = TempDir::new("config-corrupt");
        let path = dir.write("config.json", "{ not json");

        assert!(Config::load(&path).is_err());
    }
}
