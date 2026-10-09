use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use crate::core::fit::Mapping;

const SETTINGS_FILE: &str = "settings.json";
const SUPPORTED_REVISION: &str = "0.6.";

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub tablet: String,
    pub absolute: bool,
    pub mapping: Mapping,
    // Area centre in mm from the tablet's top-left corner.
    pub center: (f64, f64),
}

#[derive(Debug, Clone)]
pub struct OtdSettings {
    pub revision: String,
    pub profiles: Vec<Profile>,
    json: Value,
}

// Where an installed (non-portable) OpenTabletDriver keeps its data, if it exists.
pub fn default_data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        dirs::data_local_dir()
    } else {
        dirs::config_dir()
    };
    base.map(|dir| dir.join("OpenTabletDriver"))
        .filter(|dir| dir.join(SETTINGS_FILE).is_file())
}

// A portable OpenTabletDriver keeps its data in a `userdata` folder next to the executables;
// users may pick either that folder or the program folder.
pub fn resolve_data_dir(picked: &Path) -> PathBuf {
    let portable = picked.join("userdata");
    if portable.join(SETTINGS_FILE).is_file() {
        portable
    } else {
        picked.to_path_buf()
    }
}

impl OtdSettings {
    pub fn load(data_dir: &Path) -> Result<OtdSettings> {
        let path = data_dir.join(SETTINGS_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("parsing {}", path.display()))?;
        let profiles = json["Profiles"]
            .as_array()
            .context("settings have no profiles")?
            .iter()
            .map(profile)
            .collect::<Result<_>>()
            .with_context(|| format!("reading profiles in {}", path.display()))?;
        Ok(OtdSettings {
            revision: json["Revision"].as_str().unwrap_or_default().to_string(),
            profiles,
            json,
        })
    }

    pub fn is_supported(&self) -> bool {
        self.revision.starts_with(SUPPORTED_REVISION)
    }

    // Writes a copy of the settings with the profile's area replaced, as an OTD preset. The
    // centre is kept, moved just enough to stay on the tablet when its size is known. Existing
    // presets are never overwritten.
    pub fn write_preset(
        &self,
        data_dir: &Path,
        index: usize,
        mapping: Mapping,
        tablet_size: Option<(f64, f64)>,
        name: &str,
    ) -> Result<PathBuf> {
        ensure!(
            !name.is_empty() && !name.contains(['/', '\\', ':']) && !name.starts_with('.'),
            "invalid preset name {name:?}"
        );
        let current = self.profiles.get(index).context("no such OTD profile")?;
        let (x, y) = tablet_size.map_or(current.center, |size| {
            clamp_center(current.center, &mapping, size)
        });
        let aspect_changed = ((mapping.area_width * current.mapping.area_height)
            / (mapping.area_height * current.mapping.area_width)
            - 1.0)
            .abs()
            > 1e-9;

        let mut json = self.json.clone();
        let area = json
            .pointer_mut(&format!("/Profiles/{index}/AbsoluteModeSettings"))
            .and_then(Value::as_object_mut)
            .context("profile has no absolute mode settings")?;
        if aspect_changed {
            area.insert("LockAspectRatio".into(), false.into());
        }
        let tablet = area
            .get_mut("Tablet")
            .and_then(Value::as_object_mut)
            .context("profile has no tablet area")?;
        tablet.insert("Width".into(), mapping.area_width.into());
        tablet.insert("Height".into(), mapping.area_height.into());
        tablet.insert("X".into(), x.into());
        tablet.insert("Y".into(), y.into());
        tablet.insert("Rotation".into(), mapping.rotation.rem_euclid(360.0).into());

        let dir = data_dir.join("Presets");
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join(format!("{name}.json"));
        let file =
            File::create_new(&path).with_context(|| format!("creating {}", path.display()))?;
        serde_json::to_writer_pretty(file, &json)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }
}

fn profile(value: &Value) -> Result<Profile> {
    let number = |value: &Value, what: &str| value.as_f64().with_context(|| format!("no {what}"));
    let area = &value["AbsoluteModeSettings"];
    let tablet = &area["Tablet"];
    let display = &area["Display"];
    Ok(Profile {
        tablet: value["Tablet"].as_str().unwrap_or_default().to_string(),
        absolute: value["OutputMode"]["Path"]
            .as_str()
            .is_some_and(|path| path.contains("Absolute")),
        mapping: Mapping {
            area_width: number(&tablet["Width"], "tablet area width")?,
            area_height: number(&tablet["Height"], "tablet area height")?,
            rotation: number(&tablet["Rotation"], "tablet area rotation")?,
            display_width: number(&display["Width"], "display area width")?,
            display_height: number(&display["Height"], "display area height")?,
        },
        center: (
            number(&tablet["X"], "tablet area X")?,
            number(&tablet["Y"], "tablet area Y")?,
        ),
    })
}

// Moves the centre the least so the rotated area's bounding box stays on the tablet; an area
// larger than the tablet is centred.
fn clamp_center(center: (f64, f64), mapping: &Mapping, tablet: (f64, f64)) -> (f64, f64) {
    let (sin, cos) = mapping.rotation.to_radians().sin_cos();
    let half_x = (mapping.area_width * cos.abs() + mapping.area_height * sin.abs()) / 2.0;
    let half_y = (mapping.area_width * sin.abs() + mapping.area_height * cos.abs()) / 2.0;
    let clamp = |c: f64, half: f64, size: f64| {
        if 2.0 * half >= size {
            size / 2.0
        } else {
            c.clamp(half, size - half)
        }
    };
    (
        clamp(center.0, half_x, tablet.0),
        clamp(center.1, half_y, tablet.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;

    const SETTINGS: &str = r#"{
  "Revision": "0.6.7",
  "Profiles": [
    {
      "Tablet": "Wacom CTL-4100",
      "OutputMode": { "Path": "OpenTabletDriver.Desktop.Output.AbsoluteMode", "Settings": [], "Enable": true },
      "Filters": [ { "Path": "Some.Filter", "Settings": [ { "Property": "Latency", "Value": 4.0 } ], "Enable": false } ],
      "AbsoluteModeSettings": {
        "Display": { "Width": 1920.0, "Height": 1080.0, "X": 960.0, "Y": 540.0, "Rotation": 0.0 },
        "Tablet": { "Width": 80.0, "Height": 45.0, "X": 40.0, "Y": 22.5, "Rotation": 0.0 },
        "EnableClipping": true,
        "EnableAreaLimiting": false,
        "LockAspectRatio": true
      },
      "Bindings": { "TipButton": { "Path": "OpenTabletDriver.Desktop.Binding.MouseBinding", "Enable": true } }
    },
    {
      "Tablet": "Huion H420",
      "OutputMode": { "Path": "OpenTabletDriver.Desktop.Output.RelativeMode", "Settings": [], "Enable": true },
      "AbsoluteModeSettings": {
        "Display": { "Width": 1920.0, "Height": 1080.0, "X": 960.0, "Y": 540.0, "Rotation": 0.0 },
        "Tablet": { "Width": 101.6, "Height": 57.15, "X": 50.8, "Y": 28.575, "Rotation": 0.0 },
        "EnableClipping": true,
        "EnableAreaLimiting": false,
        "LockAspectRatio": false
      }
    }
  ],
  "LockUsableAreaTablet": true,
  "Tools": []
}"#;
    const CTL_4100: (f64, f64) = (152.0, 95.0);

    fn installed(name: &str, settings: &str) -> (TempDir, OtdSettings) {
        let dir = TempDir::new(name);
        dir.write("settings.json", settings);
        let settings = OtdSettings::load(&dir.0).unwrap();
        (dir, settings)
    }

    fn mapping(width: f64, height: f64, rotation: f64) -> Mapping {
        Mapping {
            area_width: width,
            area_height: height,
            rotation,
            display_width: 1920.0,
            display_height: 1080.0,
        }
    }

    fn written(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn loads_profiles_from_settings() {
        let (_dir, settings) = installed("otd-load", SETTINGS);

        assert!(settings.is_supported());
        assert_eq!(settings.profiles.len(), 2);
        assert_eq!(
            settings.profiles[0],
            Profile {
                tablet: "Wacom CTL-4100".to_string(),
                absolute: true,
                mapping: mapping(80.0, 45.0, 0.0),
                center: (40.0, 22.5),
            }
        );
        assert!(!settings.profiles[1].absolute);
    }

    #[test]
    fn settings_with_a_byte_order_mark_load() {
        let (_dir, settings) = installed("otd-bom", &format!("\u{feff}{SETTINGS}"));

        assert_eq!(settings.profiles.len(), 2);
    }

    #[test]
    fn other_revisions_are_flagged() {
        let (_dir, settings) = installed("otd-revision", &SETTINGS.replace("0.6.7", "0.7.0"));

        assert!(!settings.is_supported());
    }

    #[test]
    fn missing_settings_are_an_error() {
        let dir = TempDir::new("otd-missing");

        assert!(OtdSettings::load(&dir.0).is_err());
    }

    #[test]
    fn portable_install_resolves_to_userdata() {
        let dir = TempDir::new("otd-portable");
        assert_eq!(resolve_data_dir(&dir.0), dir.0);

        dir.write("userdata/settings.json", SETTINGS);

        assert_eq!(resolve_data_dir(&dir.0), dir.0.join("userdata"));
    }

    #[test]
    fn preset_changes_only_the_area() {
        let (dir, settings) = installed("otd-preset", SETTINGS);

        let path = settings
            .write_preset(&dir.0, 0, mapping(78.0, 43.875, 1.5), None, "smaller")
            .unwrap();

        let mut expected: Value = serde_json::from_str(SETTINGS).unwrap();
        expected["Profiles"][0]["AbsoluteModeSettings"]["Tablet"] = serde_json::json!({
            "Width": 78.0, "Height": 43.875, "X": 40.0, "Y": 22.5, "Rotation": 1.5
        });
        assert_eq!(path, dir.0.join("Presets/smaller.json"));
        assert_eq!(written(&path), expected);
    }

    #[test]
    fn growing_area_is_kept_on_the_tablet() {
        let (dir, settings) = installed("otd-clamp", SETTINGS);

        let path = settings
            .write_preset(
                &dir.0,
                0,
                mapping(90.0, 50.625, 0.0),
                Some(CTL_4100),
                "bigger",
            )
            .unwrap();

        let tablet = &written(&path)["Profiles"][0]["AbsoluteModeSettings"]["Tablet"];
        assert_eq!(
            (tablet["X"].as_f64(), tablet["Y"].as_f64()),
            (Some(45.0), Some(25.3125))
        );
    }

    #[test]
    fn unknown_tablet_size_keeps_the_centre() {
        let (dir, settings) = installed("otd-unknown-size", SETTINGS);

        let path = settings
            .write_preset(&dir.0, 0, mapping(90.0, 50.625, 0.0), None, "bigger")
            .unwrap();

        let tablet = &written(&path)["Profiles"][0]["AbsoluteModeSettings"]["Tablet"];
        assert_eq!(
            (tablet["X"].as_f64(), tablet["Y"].as_f64()),
            (Some(40.0), Some(22.5))
        );
    }

    #[test]
    fn clamping_accounts_for_rotation() {
        let (x, y) = clamp_center((0.0, 0.0), &mapping(80.0, 40.0, 90.0), CTL_4100);

        assert!(
            (x - 20.0).abs() < 1e-9 && (y - 40.0).abs() < 1e-9,
            "{x}, {y}"
        );
    }

    #[test]
    fn area_larger_than_the_tablet_is_centred() {
        assert_eq!(
            clamp_center((10.0, 10.0), &mapping(200.0, 50.0, 0.0), CTL_4100),
            (76.0, 25.0)
        );
    }

    #[test]
    fn changed_aspect_unlocks_the_aspect_ratio() {
        let (dir, settings) = installed("otd-aspect", SETTINGS);
        let lock = |path: &Path| {
            written(path)["Profiles"][0]["AbsoluteModeSettings"]["LockAspectRatio"].as_bool()
        };

        let same = settings
            .write_preset(&dir.0, 0, mapping(88.0, 49.5, 0.0), None, "same-aspect")
            .unwrap();
        let wider = settings
            .write_preset(&dir.0, 0, mapping(88.0, 45.0, 0.0), None, "wider")
            .unwrap();

        assert_eq!(lock(&same), Some(true));
        assert_eq!(lock(&wider), Some(false));
    }

    #[test]
    fn rotation_is_written_between_0_and_360() {
        let (dir, settings) = installed("otd-rotation", SETTINGS);

        let path = settings
            .write_preset(&dir.0, 0, mapping(80.0, 45.0, -2.0), None, "tilted")
            .unwrap();

        let tablet = &written(&path)["Profiles"][0]["AbsoluteModeSettings"]["Tablet"];
        assert_eq!(tablet["Rotation"].as_f64(), Some(358.0));
    }

    #[test]
    fn existing_presets_are_never_overwritten() {
        let (dir, settings) = installed("otd-existing", SETTINGS);
        let existing = dir.write("Presets/mine.json", "{}");

        let result = settings.write_preset(&dir.0, 0, mapping(78.0, 43.875, 0.0), None, "mine");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(existing).unwrap(), "{}");
    }

    #[test]
    fn preset_names_cannot_leave_the_presets_folder() {
        let (dir, settings) = installed("otd-names", SETTINGS);

        for name in ["", "../settings", "a/b", "C:evil", ".hidden"] {
            let result = settings.write_preset(&dir.0, 0, mapping(78.0, 43.875, 0.0), None, name);
            assert!(result.is_err(), "{name:?}");
        }
    }

    #[test]
    #[ignore = "reads the OpenTabletDriver settings on this machine (set OTD_DATA_DIR for portable installs)"]
    fn real_settings_load() {
        let data_dir = std::env::var_os("OTD_DATA_DIR")
            .map(|dir| resolve_data_dir(Path::new(&dir)))
            .or_else(default_data_dir)
            .expect("no OpenTabletDriver settings found");

        let settings = OtdSettings::load(&data_dir).unwrap();

        eprintln!("{} {:#?}", settings.revision, settings.profiles);
        assert!(!settings.profiles.is_empty());
        for profile in &settings.profiles {
            assert!(profile.mapping.area_width > 0.0 && profile.mapping.display_width > 0.0);
            eprintln!(
                "{}: size {:?}",
                profile.tablet,
                crate::core::tablets::tablet_size(&profile.tablet)
            );
        }
    }
}
