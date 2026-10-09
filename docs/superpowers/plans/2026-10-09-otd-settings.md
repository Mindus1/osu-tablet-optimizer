# OTD Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** read OpenTabletDriver's settings (profiles, area, display, mode), look up each tablet's physical size, and write a suggested area as a new OTD preset without touching `settings.json`.

**Architecture:**
- **`src/core/tablets.rs`:** looks up a tablet's active area (mm) by OTD name in a bundled `src/core/tablets.tsv`. That table is generated from OTD's own tablet configurations by `examples/update_tablets.rs`, and the file is committed.
- **`src/core/otd.rs`, reading:** loads `<data dir>/settings.json` as a `serde_json::Value`, so unknown fields survive. Each profile becomes a `Profile { tablet, absolute, mapping: fit::Mapping, center }`.
- **`src/core/otd.rs`, presets:** `write_preset` clones the JSON, replaces only the profile's `AbsoluteModeSettings.Tablet.{Width, Height, X, Y, Rotation}` (plus `LockAspectRatio = false` when the aspect changes), and writes `<data dir>/Presets/<name>.json` with create-new, so nothing is ever overwritten.
- **Keeping the centre:** the area keeps its centre, moved the least so the rotated area's bounding box stays on the tablet; an area larger than the tablet is centred. Rotation is written in [0, 360).
- **Data folder:** the platform default (`%LOCALAPPDATA%`, or `dirs::config_dir()` on Linux/macOS, + `/OpenTabletDriver`), or a picked folder. A picked program folder with `userdata/settings.json` resolves to `userdata` (portable install).

**Verified:**
- OTD 0.6.x `PresetManager` loads every `Presets/*.json` with `Settings.TryDeserialize`, so a preset is a complete settings file.
- The 359 tablet configurations in OTD 0.6.x all have `Specifications.Digitizer.{Width, Height}` (some files start with a byte-order mark) and no duplicate names. The CTL-4100 is 152×95 mm.
- The prototype loads the test machine's portable 0.6.7 settings: CTL-4100, absolute, 80×45 mm, 3840×2160 display, size found in the table.

**Tech Stack:** Rust 2024, `serde_json` (`Value`, `pointer_mut`), `dirs`, `File::create_new`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- `src/core/` is pure logic, no iced. Tests live in the module.
- `settings.json` is never written; presets are never overwritten.
- `anyhow::Result` + `.context(..)` at I/O boundaries. Never `unwrap()` on user data. JSON paths are reached with `pointer_mut`/`as_*`, never by panicking `IndexMut`.
- General-purpose: no hardcoded user paths. Real-machine checks are `#[ignore]` and take `OTD_DATA_DIR` for portable installs.
- Commits: Conventional Commits ending with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR
  ```

## Review Focus

- **`settings.json` with a UTF-8 byte-order mark:** still loads. Pinned by `settings_with_a_byte_order_mark_load` (Task 2).
- **A profile not in absolute mode:** flagged (`absolute == false`) so the UI can explain. Pinned by `loads_profiles_from_settings` (Task 2).
- **A preset name that leaves the Presets folder** (`../settings`, `a/b`, `C:evil`, empty, hidden): rejected. Pinned by `preset_names_cannot_leave_the_presets_folder` (Task 2).
- **A preset with the same name already existing:** an error, and the existing file is untouched. Pinned by `existing_presets_are_never_overwritten` (Task 2).
- **An area bigger than the tablet, or rotated:** centred, or clamped by its rotated bounding box. Pinned by `area_larger_than_the_tablet_is_centred` and `clamping_accounts_for_rotation` (Task 2).

---

### Task 1: Bundled tablet sizes

**Files:**
- Modify: `src/core.rs`, `README.md` (credits)
- Create: `src/core/tablets.rs`, `src/core/tablets.tsv` (generated), `examples/update_tablets.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn tablet_size(name: &str) -> Option<(f64, f64)>` (width, height in mm).

- [ ] **Step 1: Register the module and add the generator**

Replace `src/core.rs` with:

```rust
pub mod fit;
pub mod hits;
pub mod library;
pub mod replay;
pub mod tablets;
```

Create `examples/update_tablets.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

fn main() -> Result<()> {
    let checkout = std::env::args()
        .nth(1)
        .context("usage: cargo run --example update_tablets -- <OpenTabletDriver checkout>")?;
    let mut files = Vec::new();
    collect_json(
        &Path::new(&checkout).join("OpenTabletDriver.Configurations/Configurations"),
        &mut files,
    )?;

    let mut rows = Vec::new();
    for path in files {
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let config: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("parsing {}", path.display()))?;
        let digitizer = &config["Specifications"]["Digitizer"];
        if let (Some(name), Some(width), Some(height)) = (
            config["Name"].as_str(),
            digitizer["Width"].as_f64(),
            digitizer["Height"].as_f64(),
        ) {
            rows.push(format!("{name}\t{width}\t{height}"));
        }
    }
    rows.sort();
    rows.dedup();

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core/tablets.tsv");
    fs::write(&out, rows.join("\n") + "\n")
        .with_context(|| format!("writing {}", out.display()))?;
    println!("{} tablets written to {}", rows.len(), out.display());
    Ok(())
}

fn collect_json(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_json(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            out.push(path);
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Write the failing tests**

Create `src/core/tablets.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tablet_has_its_size() {
        assert_eq!(tablet_size("Wacom CTL-4100"), Some((152.0, 95.0)));
    }

    #[test]
    fn unknown_tablet_has_no_size() {
        assert_eq!(tablet_size("Not A Tablet"), None);
    }

    #[test]
    fn every_bundled_tablet_has_a_positive_size() {
        for line in TABLETS.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3, "{line:?}");
            let size = tablet_size(fields[0]).unwrap();
            assert!(size.0 > 0.0 && size.1 > 0.0, "{line:?}");
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::tablets`
Expected: compile errors `cannot find function tablet_size`, `cannot find value TABLETS`.

- [ ] **Step 4: Implement, then generate the table**

Add above the test module in `src/core/tablets.rs`:

```rust
// Name, width and height (mm) of every tablet OpenTabletDriver supports, generated from its
// tablet configurations by `cargo run --example update_tablets -- <OTD checkout>`.
const TABLETS: &str = include_str!("tablets.tsv");

pub fn tablet_size(name: &str) -> Option<(f64, f64)> {
    TABLETS.lines().find_map(|line| {
        let mut fields = line.split('\t');
        if fields.next()? != name {
            return None;
        }
        Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
    })
}
```

Generate the table from OTD 0.6.x (a sparse clone into a scratch folder outside the repo):

```bash
git clone -q --depth 1 --branch 0.6.x --filter=blob:none --sparse https://github.com/OpenTabletDriver/OpenTabletDriver.git "$SCRATCH/otd-src"
git -C "$SCRATCH/otd-src" sparse-checkout set OpenTabletDriver.Configurations/Configurations
touch src/core/tablets.tsv
cargo run --example update_tablets -- "$SCRATCH/otd-src"
```

Expected: `359 tablets written to …/src/core/tablets.tsv`, with a line `Wacom CTL-4100	152	95`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::tablets`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 6: README credit**

Append to `README.md`:

```markdown

## Credits

Tablet dimensions in `src/core/tablets.tsv` come from [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver)'s tablet configurations (LGPL-3.0).
```

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add src/core.rs src/core/tablets.rs src/core/tablets.tsv examples/update_tablets.rs README.md
git commit -m "feat: bundled tablet sizes from OpenTabletDriver's configurations

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 2: OTD settings and presets

**Files:**
- Modify: `src/core.rs`, `src/core/library.rs` (share the test `TempDir`)
- Create: `src/core/otd.rs`

**Interfaces:**
- Consumes: `fit::Mapping`, `library::tests::TempDir` (made `pub(crate)` here: struct, field `.0`, `new`, `write`).
- Produces:
  - `pub struct Profile { pub tablet: String, pub absolute: bool, pub mapping: Mapping, pub center: (f64, f64) }`.
  - `pub struct OtdSettings { pub revision: String, pub profiles: Vec<Profile>, json: Value }`, with `load(data_dir) -> Result<OtdSettings>`, `is_supported() -> bool` (revision `0.6.x`) and `write_preset(&self, data_dir, index, mapping, tablet_size: Option<(f64, f64)>, name) -> Result<PathBuf>`.
  - `pub fn default_data_dir() -> Option<PathBuf>` and `pub fn resolve_data_dir(picked: &Path) -> PathBuf`.

- [ ] **Step 1: Register the module and share `TempDir`**

Replace `src/core.rs` with:

```rust
pub mod fit;
pub mod hits;
pub mod library;
pub mod otd;
pub mod replay;
pub mod tablets;
```

In `src/core/library.rs` make the test helper crate-visible:
- `mod tests {` becomes `pub(crate) mod tests {`
- `struct TempDir(PathBuf);` becomes `pub(crate) struct TempDir(pub(crate) PathBuf);`
- `fn new(name: &str) -> TempDir {` becomes `pub(crate) fn new(name: &str) -> TempDir {`
- `fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {` becomes `pub(crate) fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {`

- [ ] **Step 2: Write the failing tests**

Create `src/core/otd.rs` with only:

```rust
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
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib core::otd`
Expected: compile errors `cannot find type OtdSettings / Profile / Mapping / Value`, `cannot find function resolve_data_dir / clamp_center`.

- [ ] **Step 4: Implement (above the test module)**

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::otd`
Expected: `test result: ok. 14 passed`.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add src/core.rs src/core/library.rs src/core/otd.rs
git commit -m "feat: read OTD settings and write area presets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 3: Real-settings check and docs

**Files:**
- Modify: `src/core/otd.rs` (ignored test), `CLAUDE.md` (Commands)

- [ ] **Step 1: Add the ignored test at the end of `mod tests` in `src/core/otd.rs`**

```rust
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
```

- [ ] **Step 2: Run it on this machine (portable OTD)**

Run: `OTD_DATA_DIR="<OTD program folder>" cargo test --lib core::otd -- --ignored --nocapture`
Expected: `1 passed`. The output shows revision `0.6.x`, each profile's tablet, mode and mapping, and the tablet's size from the table.

- [ ] **Step 3: Document it in CLAUDE.md**

In `CLAUDE.md`, change the `cargo test -- --ignored` line under `## Commands` to:

```sh
cargo test -- --ignored                     # real osu!/OTD installs on this machine; OTD_DATA_DIR=<folder> for portable OTD
```

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no diff, no warnings.

```bash
git add src/core/otd.rs CLAUDE.md
git commit -m "test: load the real OTD settings

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR"
```

---

### Task 4: Verify on GitHub and merge (CLAUDE.md feature flow)

- [ ] **Step 1: Local gate**: `cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked`. Expected: 97 passed, 4 ignored.
- [ ] **Step 2: Refactor new code only**, re-run the gate, and commit `refactor: ...` if anything changed.
- [ ] **Step 3: README check**: the Credits section was added in Task 1; nothing else is user-visible yet.
- [ ] **Step 4: Push, open the PR, CI green**

```bash
git push -u origin feature/otd-settings
gh pr create --base dev --title "feat: OTD settings, tablet sizes and presets" --body "core::otd reads profiles and writes area presets; core::tablets bundles OTD tablet sizes.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
gh pr checks --watch
```

- [ ] **Step 5: Whole-branch review, then squash-merge and clean up**

```bash
gh pr merge --squash --delete-branch
git switch dev && git pull && git fetch --prune
git branch -vv | grep ': gone]' || true
```

- [ ] **Step 6: Continue with feature 7.** Don't ask about dev→main until feature 9 (user's instruction).

## Notes for later features

- Feature 7 (Setup): start from `default_data_dir()`; if it's missing, ask for the OTD folder and pass the pick through `resolve_data_dir`. Show `OtdSettings::is_supported()` as a warning, and say clearly when the chosen profile isn't absolute.
- Feature 9 (Area page): `tablet_size(&profile.tablet)` drives the tablet outline, with a manual override in Setup when it's `None`. Name presets `oto-YYYY-MM-DD-{capped|full}` and add a suffix if that name exists (`write_preset` refuses overwrites). Tell users to refresh presets in OTD.
- From the final review (deferred minors), for feature 9:
  - Check `profile.absolute` before calling `write_preset`.
  - Restrict any user-typed preset name to `[A-Za-z0-9 _-]`, since Windows reserved names like `NUL` and characters like `*?"<>|` pass today.
  - Write presets via `to_vec_pretty` + `write_all`, removing the file on failure, so a failed write can't block that name.
  - Linux "Artist Mode" is absolute but is flagged non-absolute (the path lacks "Absolute").
- For feature 7: one malformed profile fails `load` for all profiles. Always show the resolved OTD data folder, since an old installed OTD may shadow a portable one.
