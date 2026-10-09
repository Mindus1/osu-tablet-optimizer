use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::fit::{Mapping, Metrics};
use crate::core::storage;

// The chart covers the points between these quantiles; outliers sit on its edge.
const RANGE_QUANTILE: f64 = 0.05;
const RANGE_MARGIN: f64 = 0.15;
// Keeps a single value (or several equal ones) off the chart's edge.
const MIN_RANGE_MARGIN: f64 = 0.01;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    // Empty until the user names it.
    pub name: String,
    pub mapping: Mapping,
    // When OpenTabletDriver saved the settings holding this area (ms since the Unix epoch).
    pub since_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub played_at_ms: i64,
    pub metrics: Metrics,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    // In the order they were logged and never removed, so an index names an area for good.
    pub snapshots: Vec<Snapshot>,
    replays: HashMap<PathBuf, Entry>,
    // Areas the user picked by hand.
    assigned: HashMap<PathBuf, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Metric {
    #[default]
    Overshoot,
    Rotation,
    AxisRatio,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub played_at_ms: i64,
    pub snapshot: Option<usize>,
    pub value: f64,
    pub samples: usize,
}

impl Metric {
    pub const ALL: [Metric; 4] = [
        Metric::Overshoot,
        Metric::Rotation,
        Metric::AxisRatio,
        Metric::Error,
    ];

    pub fn of(self, metrics: &Metrics) -> f64 {
        match self {
            Metric::Overshoot => metrics.overshoot,
            Metric::Rotation => metrics.rotation_bias,
            Metric::AxisRatio => metrics.axis_ratio,
            Metric::Error => metrics.error,
        }
    }

    // The value when aim matches the area perfectly.
    pub fn ideal(self) -> f64 {
        match self {
            Metric::AxisRatio => 1.0,
            Metric::Overshoot | Metric::Rotation | Metric::Error => 0.0,
        }
    }
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Metric::Overshoot => "Overshoot",
            Metric::Rotation => "Rotation",
            Metric::AxisRatio => "Horizontal ÷ vertical",
            Metric::Error => "Distance from the center",
        })
    }
}

impl History {
    // A missing file is an empty history.
    pub fn load(path: &Path) -> Result<History> {
        if !path.exists() {
            return Ok(History::default());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        storage::write_atomic(path, &serde_json::to_vec(self)?)
    }

    // Logs OpenTabletDriver's area when it differs from the last one logged; true when it did.
    // A change without a newer save of the settings (another profile or OTD folder picked in
    // Setup) can't be dated, so it replaces the last area instead of starting a new one.
    pub fn observe(&mut self, mapping: Mapping, saved_at_ms: i64) -> bool {
        match self.snapshots.last_mut() {
            Some(last) if last.mapping == mapping => return false,
            Some(last) if saved_at_ms <= last.since_ms => {
                last.name.clear();
                last.mapping = mapping;
            }
            _ => self.snapshots.push(Snapshot {
                name: String::new(),
                mapping,
                since_ms: saved_at_ms,
            }),
        }
        true
    }

    // The area a replay was played with: the user's pick, else the last one saved before it was
    // played. None when it was played before the first logged area.
    pub fn snapshot_of(&self, path: &Path, played_at_ms: i64) -> Option<usize> {
        let assigned = self
            .assigned
            .get(path)
            .copied()
            .filter(|&index| index < self.snapshots.len());
        assigned.or_else(|| {
            self.snapshots
                .iter()
                .enumerate()
                .filter(|(_, snapshot)| snapshot.since_ms <= played_at_ms)
                .max_by_key(|&(index, snapshot)| (snapshot.since_ms, index))
                .map(|(index, _)| index)
        })
    }

    // None hands the replays back to their date.
    pub fn assign(&mut self, paths: impl IntoIterator<Item = PathBuf>, snapshot: Option<usize>) {
        for path in paths {
            match snapshot {
                Some(index) => {
                    self.assigned.insert(path, index);
                }
                None => {
                    self.assigned.remove(&path);
                }
            }
        }
    }

    // How many of these replays were played with a logged area other than the current (last)
    // one. Unknown ones don't count: they were probably played with the current area.
    pub fn from_other_areas(&self, replays: &[(PathBuf, Entry)]) -> usize {
        let current = self.snapshots.len().checked_sub(1);
        replays
            .iter()
            .filter(|(path, entry)| {
                self.snapshot_of(path, entry.played_at_ms)
                    .is_some_and(|area| Some(area) != current)
            })
            .count()
    }

    pub fn record(&mut self, measured: &[(PathBuf, Entry)]) {
        self.replays.extend(measured.iter().cloned());
    }

    // Every recorded replay, oldest first.
    pub fn points(&self, metric: Metric) -> Vec<Point> {
        let mut points: Vec<Point> = self
            .replays
            .iter()
            .map(|(path, entry)| Point {
                played_at_ms: entry.played_at_ms,
                snapshot: self.snapshot_of(path, entry.played_at_ms),
                value: metric.of(&entry.metrics),
                samples: entry.metrics.samples,
            })
            .collect();
        points.sort_by_key(|point| point.played_at_ms);
        points
    }
}

// How many replays one area has, and their mean weighted by usable jumps so short replays
// count less.
pub fn average(points: &[Point], snapshot: Option<usize>) -> Option<(usize, f64)> {
    let from: Vec<&Point> = points
        .iter()
        .filter(|point| point.snapshot == snapshot)
        .collect();
    let samples: usize = from.iter().map(|point| point.samples).sum();
    if samples == 0 {
        return None;
    }
    let total: f64 = from
        .iter()
        .map(|point| point.value * point.samples as f64)
        .sum();
    Some((from.len(), total / samples as f64))
}

// The chart's value range: most points and the ideal value, with a margin.
pub fn chart_range(points: &[Point], ideal: f64) -> (f64, f64) {
    let mut values: Vec<f64> = points.iter().map(|point| point.value).collect();
    values.sort_by(f64::total_cmp);
    let at = |q: f64| {
        let index = (values.len().saturating_sub(1) as f64 * q).round() as usize;
        values.get(index).copied().unwrap_or(ideal)
    };
    let low = at(RANGE_QUANTILE).min(ideal);
    let high = at(1.0 - RANGE_QUANTILE).max(ideal);
    let margin = ((high - low) * RANGE_MARGIN).max(MIN_RANGE_MARGIN);
    (low - margin, high + margin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::library::tests::TempDir;

    fn mapping(width: f64) -> Mapping {
        Mapping {
            area_width: width,
            area_height: 45.0,
            rotation: 0.0,
            display_width: 1920.0,
            display_height: 1080.0,
        }
    }

    fn entry(played_at_ms: i64, overshoot: f64, samples: usize) -> Entry {
        Entry {
            played_at_ms,
            metrics: Metrics {
                samples,
                overshoot,
                rotation_bias: 0.0,
                axis_ratio: 1.0,
                error: 0.5,
            },
        }
    }

    fn point(snapshot: Option<usize>, value: f64, samples: usize) -> Point {
        Point {
            played_at_ms: 0,
            snapshot,
            value,
            samples,
        }
    }

    // One area from t = 100, another from t = 200.
    fn two_areas() -> History {
        let mut history = History::default();
        history.observe(mapping(80.0), 100);
        history.observe(mapping(78.0), 200);
        history
    }

    #[test]
    fn only_area_changes_are_logged() {
        let mut history = History::default();

        assert!(history.observe(mapping(80.0), 100));
        assert!(!history.observe(mapping(80.0), 150));
        assert!(history.observe(mapping(78.0), 200));
        assert!(history.observe(mapping(80.0), 300));

        let since: Vec<i64> = history.snapshots.iter().map(|s| s.since_ms).collect();
        assert_eq!(since, [100, 200, 300]);
        assert!(history.snapshots.iter().all(|s| s.name.is_empty()));
    }

    #[test]
    fn an_area_change_without_a_new_save_replaces_the_last_area() {
        let mut history = two_areas();
        history.snapshots[1].name = "named".to_string();

        // Another OTD folder with older settings, then another profile of the same settings.
        assert!(history.observe(mapping(70.0), 150));
        assert!(history.observe(mapping(72.0), 200));

        assert_eq!(history.snapshots.len(), 2);
        assert_eq!(
            history.snapshots[1],
            Snapshot {
                name: String::new(),
                mapping: mapping(72.0),
                since_ms: 200,
            }
        );
        assert_eq!(history.snapshot_of(Path::new("a.osr"), 999), Some(1));
    }

    #[test]
    fn replays_belong_to_the_last_area_saved_before_them() {
        let history = two_areas();
        let path = Path::new("a.osr");

        assert_eq!(history.snapshot_of(path, 50), None);
        assert_eq!(history.snapshot_of(path, 150), Some(0));
        assert_eq!(history.snapshot_of(path, 200), Some(1));
        assert_eq!(history.snapshot_of(path, 999), Some(1));
    }

    #[test]
    fn assigned_area_overrides_the_date() {
        let mut history = two_areas();
        let path = PathBuf::from("a.osr");

        history.assign([path.clone()], Some(0));
        assert_eq!(history.snapshot_of(&path, 999), Some(0));

        history.assign([path.clone()], None);
        assert_eq!(history.snapshot_of(&path, 999), Some(1));
    }

    #[test]
    fn assignment_to_a_missing_area_falls_back_to_the_date() {
        let mut history = two_areas();
        let path = PathBuf::from("a.osr");

        history.assign([path.clone()], Some(7));

        assert_eq!(history.snapshot_of(&path, 150), Some(0));
    }

    #[test]
    fn replays_from_older_areas_are_counted() {
        let mut history = two_areas();
        history.assign([PathBuf::from("assigned.osr")], Some(0));
        let replays = [
            (PathBuf::from("unknown.osr"), entry(50, 0.0, 30)),
            (PathBuf::from("old.osr"), entry(150, 0.0, 30)),
            (PathBuf::from("assigned.osr"), entry(250, 0.0, 30)),
            (PathBuf::from("current.osr"), entry(250, 0.0, 30)),
        ];

        assert_eq!(history.from_other_areas(&replays), 2);
    }

    #[test]
    fn points_are_oldest_first_and_replace_earlier_records() {
        let mut history = two_areas();
        history.record(&[
            (PathBuf::from("late.osr"), entry(250, 0.01, 40)),
            (PathBuf::from("early.osr"), entry(150, 0.02, 30)),
        ]);
        history.record(&[(PathBuf::from("late.osr"), entry(250, -0.03, 40))]);

        assert_eq!(
            history.points(Metric::Overshoot),
            [
                Point {
                    played_at_ms: 150,
                    snapshot: Some(0),
                    value: 0.02,
                    samples: 30
                },
                Point {
                    played_at_ms: 250,
                    snapshot: Some(1),
                    value: -0.03,
                    samples: 40
                },
            ]
        );
        assert_eq!(history.points(Metric::AxisRatio)[0].value, 1.0);
    }

    #[test]
    fn history_survives_save_and_load() {
        let dir = TempDir::new("history");
        let path = dir.0.join("history.json");
        let mut history = two_areas();
        history.snapshots[0].name = "old area".to_string();
        history.record(&[(PathBuf::from("a.osr"), entry(150, 0.02, 30))]);
        history.assign([PathBuf::from("a.osr")], Some(1));

        history.save(&path).unwrap();

        assert_eq!(History::load(&path).unwrap(), history);
    }

    #[test]
    fn missing_history_is_empty() {
        let dir = TempDir::new("history-missing");

        assert_eq!(
            History::load(&dir.0.join("history.json")).unwrap(),
            History::default()
        );
    }

    #[test]
    fn corrupt_history_is_an_error() {
        let dir = TempDir::new("history-corrupt");
        let path = dir.write("history.json", "{ not json");

        assert!(History::load(&path).is_err());
    }

    #[test]
    fn average_weighs_replays_by_their_jumps() {
        let points = [
            point(Some(0), 0.0, 10),
            point(Some(0), 4.0, 30),
            point(Some(1), 100.0, 50),
        ];

        assert_eq!(average(&points, Some(0)), Some((2, 3.0)));
        assert_eq!(average(&points, None), None);
    }

    #[test]
    fn chart_range_leaves_out_outliers_but_shows_the_ideal() {
        let mut points: Vec<Point> = (0..100)
            .map(|i| point(None, 1.0 + f64::from(i) / 100.0, 1))
            .collect();
        points.push(point(None, 50.0, 1));

        let (low, high) = chart_range(&points, 0.0);

        assert!(low < 0.0 && high > 1.9 && high < 3.0, "{low} {high}");
    }

    #[test]
    fn chart_range_is_never_empty() {
        let (low, high) = chart_range(&[point(None, 0.0, 1)], 0.0);
        assert!(low < 0.0 && high > 0.0);

        let (low, high) = chart_range(&[], 1.0);
        assert!(low < 1.0 && high > 1.0);
    }
}
