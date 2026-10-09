# Analysis Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the Analysis page. "Analyze N selected" reads and fits the selected replays on a worker thread. The page shows:
- **Key numbers with 90% intervals:** overshoot, rotation, horizontal ÷ vertical, and shear.
- **Error scatter:** each jump's error along and across the move, with the hit circle drawn.
- **Polar plot:** distance gain and turn per move direction, magnified 5×.
- **Skipped replays,** each with its reason.

**Architecture:**
- **Core `aim` (tested):** `analyse(paths, beatmaps, mapping, settings) -> Report` composes `hits::replay_samples` and `fit::suggest`. It keeps the usable samples as aligned errors (along/across in radii) and 16 direction sectors (mean gain and turn). `fit::is_usable` becomes public so the charts use the fit's own filter.
- **UI `ui::analysis`:** the page plus `start(app)`, which spawns a thread and reports back through a `oneshot`; a dead worker becomes `Failed`.
- **UI `ui::charts`:** two `canvas::Program`s.
- **Without OTD:** the fit uses a neutral mapping. The gain metrics shown here don't depend on it; the area suggestion is the Area page's job.

**Verified with a prototype of exactly this code on the test machine:** 119 tests pass and clippy is clean. Over the last 30 days the page showed 92 replays analysed, 0 skipped and 9,760 jumps used:
- overshoot −4.2% (90%: −4.88% to −3.64%), "jumps land short";
- rotation +0.43° (+0.36° to +0.51°), "moves turn clockwise";
- horizontal ÷ vertical 0.978 (0.970 to 0.980);
- shear +0.05°.

The scatter showed the cloud around the hit circle. The polar outline sat inside the ring and was taller than wide, which matches the undershoot and the shorter horizontal moves. A window-only screenshot was used for the check.

**Tech Stack:** iced 0.14 with the `canvas` feature, and `iced::futures::channel::oneshot`.

**Spec:** `docs/superpowers/specs/2026-10-09-osu-tablet-optimizer-design.md`

## Global Constraints

- Chart data is computed in core and tested. Canvas drawing stays in the UI and is not unit-tested (CLAUDE.md).
- Analysis runs off the UI thread.
- Commits: Conventional Commits ending with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01AJrJtwKa9zXjmTF6wLttJR
  ```

## Review Focus

- **Replays that can't be analysed** (beatmap missing, unsupported mods, other mode): listed as skipped with their reason, never crashing the run. Pinned by `unreadable_replays_are_listed_with_their_reason` (Task 1).
- **Error sign conventions:** +along means overshoot and +across means the clockwise side. Wrong signs would mislabel the chart. Pinned by `errors_are_measured_along_and_across_the_move` (Task 1).
- **Turn sign:** clockwise turns are positive, matching `fit`'s rotation bias. Pinned by `turned_moves_show_as_clockwise_turn` (Task 1).
- **Directions with no samples:** drawn as nothing (not as gain 0). Pinned by `sectors_average_gain_and_turn_per_direction` (`samples == 0`) plus `Polar` skipping empty sectors (Task 2).
- **The worker thread dying:** the page leaves "Analysing…" and shows a notice (`Message::Failed`, Task 2).

---

### Task 1: Chart data in core

**Files:**
- Modify: `src/core.rs`, `src/core/fit.rs`
- Create: `src/core/aim.rs`

**Interfaces:**
- `pub struct Sector { direction, gain, turn: f64, samples: usize }`.
- `pub struct Report { analysed: usize, skipped: Vec<(PathBuf, String)>, errors: Vec<(f64, f64)>, sectors: Vec<Sector>, suggestion: Option<fit::Suggestion> }`.
- `pub fn analyse(&[PathBuf], &HashMap<&str, library::Beatmap>, fit::Mapping, &fit::Settings) -> Report`.
- `fit::is_usable` becomes `pub`.

- [ ] **Step 1:** Apply the diffs:

```diff
--- a/src/core.rs
+++ b/src/core.rs
@@ -1,3 +1,4 @@
+pub mod aim;
 pub mod config;
 pub mod fit;
 pub mod hits;
--- a/src/core/fit.rs
+++ b/src/core/fit.rs
@@ -243,7 +243,7 @@ fn is_valid(mapping: &Mapping) -> bool {
     sizes.iter().all(|size| size.is_finite() && *size > 0.0) && mapping.rotation.is_finite()
 }
 
-fn is_usable(sample: &Sample, settings: &Settings) -> bool {
+pub fn is_usable(sample: &Sample, settings: &Settings) -> bool {
     let (ux, uy) = sample.intended;
     let (vx, vy) = sample.actual;
     sample.radius > 0.0
```

- [ ] **Step 2: Write the failing tests.** Create `src/core/aim.rs` with only its test module from Step 4.
- [ ] **Step 3:** Run `cargo test --lib core::aim`. Expected: compile errors (`Sector`, `analyse` and so on are missing).
- [ ] **Step 4: Implement.** Final `src/core/aim.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::fit::{self, Mapping, Settings, Suggestion};
use crate::core::hits::{self, Sample};
use crate::core::library;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sector {
    // Centre of the movement-direction sector, degrees clockwise from right (screen y points down).
    pub direction: f64,
    // Mean distance travelled along the intended direction per intended distance.
    pub gain: f64,
    // Mean angle (degrees) by which moves came out turned clockwise.
    pub turn: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub analysed: usize,
    pub skipped: Vec<(PathBuf, String)>,
    // Usable samples only, as (along, across) the move in circle radii; +along is overshoot,
    // +across is to the clockwise side.
    pub errors: Vec<(f64, f64)>,
    pub sectors: Vec<Sector>,
    pub suggestion: Option<Suggestion>,
}

pub fn analyse(
    paths: &[PathBuf],
    beatmaps: &HashMap<&str, library::Beatmap<'_>>,
    mapping: Mapping,
    settings: &Settings,
) -> Report {
    let mut replays = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        match hits::replay_samples(path, beatmaps) {
            Ok(samples) => replays.push(samples),
            Err(error) => skipped.push((path.clone(), format!("{error:#}"))),
        }
    }
    let usable: Vec<Sample> = replays
        .iter()
        .flatten()
        .copied()
        .filter(|sample| fit::is_usable(sample, settings))
        .collect();
    Report {
        analysed: replays.len(),
        skipped,
        errors: usable.iter().map(aligned_error).collect(),
        sectors: sectors(&usable, 16),
        suggestion: fit::suggest(&replays, mapping, settings),
    }
}

fn aligned_error(sample: &Sample) -> (f64, f64) {
    let (ux, uy) = sample.intended;
    let (ex, ey) = (sample.actual.0 - ux, sample.actual.1 - uy);
    let length = ux.hypot(uy) * sample.radius;
    ((ex * ux + ey * uy) / length, (ey * ux - ex * uy) / length)
}

fn sectors(samples: &[Sample], count: usize) -> Vec<Sector> {
    let width = 360.0 / count as f64;
    let mut sums = vec![(0.0, 0.0, 0); count];
    for sample in samples {
        let (ux, uy) = sample.intended;
        let (vx, vy) = sample.actual;
        let direction = uy.atan2(ux).to_degrees().rem_euclid(360.0);
        let index = ((direction + width / 2.0) / width) as usize % count;
        let sum = &mut sums[index];
        sum.0 += (vx * ux + vy * uy) / (ux * ux + uy * uy);
        sum.1 += (ux * vy - uy * vx).atan2(ux * vx + uy * vy).to_degrees();
        sum.2 += 1;
    }
    sums.into_iter()
        .enumerate()
        .map(|(index, (gain, turn, samples))| {
            let mean = |total: f64| {
                if samples == 0 {
                    0.0
                } else {
                    total / samples as f64
                }
            };
            Sector {
                direction: index as f64 * width,
                gain: mean(gain),
                turn: mean(turn),
                samples,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::replay::tests::Osr;

    fn sample(intended: (f64, f64), actual: (f64, f64)) -> Sample {
        Sample {
            intended,
            actual,
            radius: 30.0,
        }
    }

    #[test]
    fn errors_are_measured_along_and_across_the_move() {
        let overshoot = aligned_error(&sample((0.0, 200.0), (0.0, 230.0)));
        let clockwise = aligned_error(&sample((200.0, 0.0), (200.0, 15.0)));

        assert_eq!(overshoot, (1.0, 0.0));
        assert_eq!(clockwise, (0.0, 0.5));
    }

    #[test]
    fn sectors_average_gain_and_turn_per_direction() {
        let samples = [
            sample((200.0, 0.0), (220.0, 0.0)),
            sample((200.0, 0.0), (180.0, 0.0)),
            sample((0.0, 200.0), (0.0, 190.0)),
        ];

        let sectors = sectors(&samples, 4);

        assert_eq!(sectors[0].samples, 2);
        assert!((sectors[0].gain - 1.0).abs() < 1e-12);
        assert_eq!(sectors[1].direction, 90.0);
        assert!((sectors[1].gain - 0.95).abs() < 1e-12);
        assert_eq!(sectors[2].samples, 0);
        assert_eq!(sectors[2].gain, 0.0);
    }

    #[test]
    fn turned_moves_show_as_clockwise_turn() {
        let (sin, cos) = 3.0_f64.to_radians().sin_cos();
        let samples = [sample((200.0, 0.0), (200.0 * cos, 200.0 * sin))];

        assert!((sectors(&samples, 16)[0].turn - 3.0).abs() < 1e-9);
    }

    #[test]
    fn unreadable_replays_are_listed_with_their_reason() {
        let path = std::env::temp_dir().join(format!("oto-aim-{}.osr", std::process::id()));
        std::fs::write(&path, Osr::default().bytes()).unwrap();
        let mapping = Mapping {
            area_width: 80.0,
            area_height: 45.0,
            rotation: 0.0,
            display_width: 1920.0,
            display_height: 1080.0,
        };

        let report = analyse(
            std::slice::from_ref(&path),
            &HashMap::new(),
            mapping,
            &Settings::default(),
        );
        std::fs::remove_file(&path).unwrap();

        assert_eq!(report.analysed, 0);
        assert_eq!(
            report.skipped,
            [(path, "beatmap not found in the osu! library".to_string())]
        );
        assert_eq!(report.suggestion, None);
    }
}
```

- [ ] **Step 5:** Run `cargo test --lib core::aim`. Expected: 4 passed. Then run the fmt and clippy gate.
- [ ] **Step 6: Commit** `feat: chart data for the aim analysis`.

---

### Task 2: Analysis page and charts

**Files:**
- Modify: `Cargo.toml` (iced `canvas`), `src/app.rs`, `src/ui.rs`, `src/ui/replays.rs` (*Analyze* starts the run), `src/ui/setup.rs` (`profile(app)`)
- Create: `src/ui/analysis.rs`, `src/ui/charts.rs`

- [ ] **Step 1:** Apply the diffs:

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -8,7 +8,7 @@ publish = false
 anyhow = "1"
 chrono = "0.4"
 dirs = "7"
-iced = "0.14"
+iced = { version = "0.14", features = ["canvas"] }
 lzma-rs = "0.3"
 md5 = "0.8"
 rfd = { version = "0.17", default-features = false, features = ["xdg-portal"] }
--- a/src/ui.rs
+++ b/src/ui.rs
@@ -1,2 +1,4 @@
+pub mod analysis;
+pub mod charts;
 pub mod replays;
 pub mod setup;
--- a/src/app.rs
+++ b/src/app.rs
@@ -9,7 +9,7 @@ use osu_tablet_optimizer::core::library::Library;
 use osu_tablet_optimizer::core::otd::OtdSettings;
 use osu_tablet_optimizer::core::storage;
 
-use crate::ui::{replays, setup};
+use crate::ui::{analysis, replays, setup};
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
 pub enum Page {
@@ -49,6 +49,7 @@ pub enum Message {
     FileDropped(PathBuf),
     Replays(replays::Message),
     Setup(setup::Message),
+    Analysis(analysis::Message),
 }
 
 #[derive(Debug, Clone)]
@@ -74,6 +75,7 @@ pub struct App {
     pub otd: Result<OtdSettings, String>,
     pub replays: replays::State,
     pub setup: setup::State,
+    pub analysis: analysis::State,
     pub notice: Option<String>,
 }
 
@@ -99,6 +101,7 @@ impl App {
             scanning: Some((0, 0)),
             rescan_pending: false,
             replays: replays::State::default(),
+            analysis: analysis::State::default(),
             notice,
         };
         app.reload_otd();
@@ -144,6 +147,7 @@ impl App {
             Message::FileDropped(path) => replays::add_files(self, vec![path]),
             Message::Replays(message) => return replays::update(self, message),
             Message::Setup(message) => return setup::update(self, message),
+            Message::Analysis(message) => return analysis::update(self, message),
         }
         Task::none()
     }
@@ -165,6 +169,7 @@ impl App {
         let page = match self.page {
             Page::Replays => replays::view(self),
             Page::Setup => setup::view(self),
+            Page::Analysis => analysis::view(self),
             other => text(other.label()).size(24).into(),
         };
         let mut content = column![page].spacing(8);
--- a/src/ui/replays.rs
+++ b/src/ui/replays.rs
@@ -11,6 +11,7 @@ use osu_tablet_optimizer::core::library::Library;
 use osu_tablet_optimizer::core::listing::{self, Period, Row};
 
 use crate::app::{self, App, Page};
+use crate::ui::analysis;
 
 // ponytail: a plain column of rows; switch to a virtualized list if 500 rows feel slow.
 const MAX_ROWS: usize = 500;
@@ -78,7 +79,10 @@ pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
             });
         }
         Message::FilesPicked(paths) => add_files(app, paths),
-        Message::Analyze => app.page = Page::Analysis,
+        Message::Analyze => {
+            app.page = Page::Analysis;
+            return analysis::start(app);
+        }
     }
     Task::none()
 }
--- a/src/ui/setup.rs
+++ b/src/ui/setup.rs
@@ -4,7 +4,7 @@ use std::path::PathBuf;
 use iced::widget::{button, column, pick_list, row, scrollable, text, text_input};
 use iced::{Center, Element, Task};
 use osu_tablet_optimizer::core::config::Config;
-use osu_tablet_optimizer::core::{library, tablets};
+use osu_tablet_optimizer::core::{library, otd, tablets};
 
 use crate::app::{self, App};
 
@@ -346,6 +346,10 @@ fn heading(label: &str) -> Element<'_, app::Message> {
     text(label).size(20).into()
 }
 
+pub fn profile(app: &App) -> Option<&otd::Profile> {
+    app.otd.as_ref().ok()?.profiles.get(app.config.profile)
+}
+
 // The size in use: the user's own entry first, then the bundled table.
 pub fn tablet_size(app: &App) -> Option<(f64, f64)> {
     app.config.tablet_size.or_else(|| {
```

- [ ] **Step 2:** Create `src/ui/charts.rs`:

```rust
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};
use osu_tablet_optimizer::core::aim::Sector;

// Errors within this many radii are drawn; the circle marks one radius (a hit).
const SCATTER_RANGE: f32 = 3.0;
// Gain and turn differences are a few percent or degrees; magnify them so the shape shows.
const MAGNIFICATION: f64 = 5.0;

pub struct Scatter<'a> {
    pub errors: &'a [(f64, f64)],
}

pub struct Polar<'a> {
    pub sectors: &'a [Sector],
}

impl<Message> canvas::Program<Message> for Scatter<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let center = frame.center();
        let scale = bounds.width.min(bounds.height) / 2.0 / SCATTER_RANGE;
        let axis = Stroke::default()
            .with_color(palette.background.strong.color)
            .with_width(1.0);
        frame.stroke(
            &Path::line(
                Point::new(0.0, center.y),
                Point::new(bounds.width, center.y),
            ),
            axis,
        );
        frame.stroke(
            &Path::line(
                Point::new(center.x, 0.0),
                Point::new(center.x, bounds.height),
            ),
            axis,
        );
        let dot = Color {
            a: 0.3,
            ..palette.background.base.text
        };
        for &(along, across) in self.errors {
            let (x, y) = (along as f32, across as f32);
            if x.abs() <= SCATTER_RANGE && y.abs() <= SCATTER_RANGE {
                let point = Point::new(center.x + x * scale - 1.0, center.y + y * scale - 1.0);
                frame.fill_rectangle(point, Size::new(2.0, 2.0), dot);
            }
        }
        frame.stroke(
            &Path::circle(center, scale),
            Stroke::default()
                .with_color(palette.primary.base.color)
                .with_width(1.5),
        );
        label(
            &mut frame,
            Point::new(4.0, center.y - 18.0),
            "undershoot",
            theme,
        );
        label(
            &mut frame,
            Point::new(bounds.width - 76.0, center.y - 18.0),
            "overshoot",
            theme,
        );
        vec![frame.into_geometry()]
    }
}

impl<Message> canvas::Program<Message> for Polar<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let center = frame.center();
        let ring = bounds.width.min(bounds.height) / 2.0 * 0.7;
        frame.stroke(
            &Path::circle(center, ring),
            Stroke::default()
                .with_color(palette.background.strong.color)
                .with_width(1.0),
        );
        let points: Vec<Point> = self
            .sectors
            .iter()
            .filter(|sector| sector.samples > 0)
            .map(|sector| {
                let angle = (sector.direction + sector.turn * MAGNIFICATION).to_radians();
                let radius = ring * (1.0 + (sector.gain - 1.0) * MAGNIFICATION).max(0.0) as f32;
                Point::new(
                    center.x + radius * angle.cos() as f32,
                    center.y + radius * angle.sin() as f32,
                )
            })
            .collect();
        if let Some((first, rest)) = points.split_first() {
            let outline = Path::new(|builder| {
                builder.move_to(*first);
                for point in rest {
                    builder.line_to(*point);
                }
                builder.close();
            });
            frame.stroke(
                &outline,
                Stroke::default()
                    .with_color(palette.primary.base.color)
                    .with_width(2.0),
            );
        }
        vec![frame.into_geometry()]
    }
}

fn label(frame: &mut Frame, position: Point, content: &str, theme: &Theme) {
    frame.fill_text(Text {
        content: content.to_string(),
        position,
        color: theme.extended_palette().background.base.text,
        size: 14.0.into(),
        ..Text::default()
    });
}
```

- [ ] **Step 3:** Create `src/ui/analysis.rs`:

```rust
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{button, canvas, column, row, scrollable, space, text};
use iced::{Center, Element, Task};
use osu_tablet_optimizer::core::aim::{self, Report};
use osu_tablet_optimizer::core::fit::{Estimate, Mapping};

use crate::app::{self, App};
use crate::ui::{charts, setup};

// Stands in for OpenTabletDriver's mapping when none is set up; the gain metrics shown here
// don't depend on it, only the area suggestion (Area page) does.
const NEUTRAL_MAPPING: Mapping = Mapping {
    area_width: 100.0,
    area_height: 56.25,
    rotation: 0.0,
    display_width: 1920.0,
    display_height: 1080.0,
};
const CHART_SIZE: f32 = 360.0;

#[derive(Debug, Default)]
pub struct State {
    running: bool,
    pub report: Option<Arc<Report>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Run,
    Done(Arc<Report>),
    Failed,
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    match message {
        Message::Run => return start(app),
        Message::Done(report) => {
            app.analysis.running = false;
            app.analysis.report = Some(report);
        }
        Message::Failed => {
            app.analysis.running = false;
            app.notice = Some("The analysis failed unexpectedly.".to_string());
        }
    }
    Task::none()
}

// Reads and fits the selected replays on a worker thread.
pub fn start(app: &mut App) -> Task<app::Message> {
    if app.analysis.running || app.replays.selected.is_empty() {
        return Task::none();
    }
    app.analysis.running = true;
    let library = Arc::clone(&app.library);
    let paths: Vec<PathBuf> = app.replays.selected.iter().cloned().collect();
    let mapping = setup::profile(app).map_or(NEUTRAL_MAPPING, |profile| profile.mapping);
    let settings = app.config.fit;
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let report = aim::analyse(&paths, &library.beatmaps_by_md5(), mapping, &settings);
        let _ = sender.send(Arc::new(report));
    });
    Task::perform(receiver, |report| {
        app::Message::Analysis(report.map_or(Message::Failed, Message::Done))
    })
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let state = &app.analysis;
    let again = button("Analyse selected again").on_press_maybe(
        (!state.running && !app.replays.selected.is_empty())
            .then_some(app::Message::Analysis(Message::Run)),
    );
    let Some(report) = &state.report else {
        let hint = if state.running {
            "Analysing…"
        } else {
            "Select replays on the Replays page, then press Analyze."
        };
        return column![text(hint), again].spacing(12).into();
    };

    let summary = text(format!(
        "{} replays analysed, {} skipped, {} jumps used.{}",
        report.analysed,
        report.skipped.len(),
        report.errors.len(),
        if state.running {
            " Analysing again…"
        } else {
            ""
        }
    ));
    let numbers: Element<'_, app::Message> = match &report.suggestion {
        Some(suggestion) => row![
            metric(
                "Overshoot",
                format!("{:+.1}%", suggestion.overshoot.value * 100.0),
                format!(
                    "90%: {:+.2}% to {:+.2}%",
                    suggestion.overshoot.low * 100.0,
                    suggestion.overshoot.high * 100.0
                ),
                verdict(
                    suggestion.overshoot,
                    0.0,
                    "jumps land long",
                    "jumps land short"
                ),
            ),
            metric(
                "Rotation",
                format!("{:+.2}°", suggestion.rotation_bias.value),
                format!(
                    "90%: {:+.2}° to {:+.2}°",
                    suggestion.rotation_bias.low, suggestion.rotation_bias.high
                ),
                verdict(
                    suggestion.rotation_bias,
                    0.0,
                    "moves turn clockwise",
                    "moves turn counter-clockwise",
                ),
            ),
            metric(
                "Horizontal ÷ vertical",
                format!("{:.3}", suggestion.axis_ratio.value),
                format!(
                    "90%: {:.3} to {:.3}",
                    suggestion.axis_ratio.low, suggestion.axis_ratio.high
                ),
                verdict(
                    suggestion.axis_ratio,
                    1.0,
                    "horizontal moves run longer",
                    "horizontal moves run shorter",
                ),
            ),
            metric(
                "Shear",
                format!("{:+.2}°", suggestion.shear),
                String::new(),
                "skew OpenTabletDriver can't correct".to_string(),
            ),
        ]
        .spacing(32)
        .into(),
        None => {
            text("Not enough data for numbers: select at least 5 replays with jumps (50 usable).")
                .into()
        }
    };
    let charts = row![
        chart(
            "Each jump's error, along and across the move (circle = hit)",
            canvas(charts::Scatter {
                errors: &report.errors
            })
        ),
        chart(
            "Distance and turn by move direction (ring = perfect, magnified 5×)",
            canvas(charts::Polar {
                sectors: &report.sectors
            })
        ),
    ]
    .spacing(32);
    let skipped = column(report.skipped.iter().take(20).map(|(path, reason)| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        text(format!("Skipped {name}: {reason}")).size(14).into()
    }))
    .spacing(4);

    scrollable(
        column![
            row![summary, space::horizontal(), again].align_y(Center),
            numbers,
            charts,
            skipped
        ]
        .spacing(24),
    )
    .into()
}

fn metric<'a>(
    title: &'a str,
    value: String,
    interval: String,
    verdict: String,
) -> Element<'a, app::Message> {
    column![
        text(title).size(14),
        text(value).size(28),
        text(interval).size(14),
        text(verdict).size(14)
    ]
    .spacing(2)
    .into()
}

fn chart<'a>(
    title: &'a str,
    canvas: iced::widget::Canvas<impl canvas::Program<app::Message> + 'a, app::Message>,
) -> Element<'a, app::Message> {
    column![
        text(title).size(14),
        canvas.width(CHART_SIZE).height(CHART_SIZE)
    ]
    .spacing(8)
    .into()
}

fn verdict(estimate: Estimate, neutral: f64, above: &str, below: &str) -> String {
    if estimate.contains(neutral) {
        "no clear effect".to_string()
    } else if estimate.value > neutral {
        above.to_string()
    } else {
        below.to_string()
    }
}
```

- [ ] **Step 4: Gate.** Run `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- [ ] **Step 5: Run it.**
  - Select replays and press *Analyze*. The page shows "Analysing…", then the numbers, both charts and any skipped replays.
  - With fewer than 5 usable replays it shows "Not enough data for numbers…".
  - For a screenshot without clicking, temporarily make `Done` select the listed rows and call `analysis::start`. Revert that edit afterwards. Capture only the app window.
- [ ] **Step 6: Commit** `feat: Analysis page with key numbers, error scatter and polar plot`.

---

### Task 3: Docs, verify on GitHub, merge

- [ ] **README Usage:** add an **Analysis** step that explains each number and both charts.
- [ ] **Gate, PR, CI, whole-branch review, then squash-merge and clean up.**
- [ ] **Continue with feature 9** (Area page). After it merges, ask "Merge dev into main?" again; that's the planned 0.1.0 release.

## Notes for later features

- Feature 9: reuse `app.analysis.report`'s `suggestion` (computed with the real OTD mapping when a profile is set) for the Area page. When the analysis ran without OTD, the area numbers must not be shown; rerun after choosing the OTD folder.
- Deferred from feature 7: hidden selections still count. Decide in feature 9 whether *Analyze* should use only the visible selection.
