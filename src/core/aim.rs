use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::fit::{self, Mapping, Settings, Suggestion};
use crate::core::history::Entry;
use crate::core::hits::{self, Sample};
use crate::core::library;

// Fewer jumps than this in a direction make its mean too noisy to draw.
const MIN_SECTOR_SAMPLES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sector {
    // Center of the movement-direction sector, degrees clockwise from right (screen y points down).
    pub direction: f64,
    // Mean distance travelled along the intended direction per intended distance, without the
    // fit's edge stop, so it agrees with the overshoot number.
    pub gain: f64,
    // Mean angle (degrees) by which moves came out turned clockwise.
    pub turn: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub analyzed: usize,
    pub skipped: Vec<(PathBuf, String)>,
    // Each replay's own numbers, for the history; replays with too few jumps have none.
    pub measured: Vec<(PathBuf, Entry)>,
    // Usable samples only, as (along, across) the move in circle radii; +along is overshoot,
    // +across is to the clockwise side.
    pub errors: Vec<(f64, f64)>,
    pub sectors: Vec<Sector>,
    pub suggestion: Option<Suggestion>,
}

pub fn analyze(
    paths: &[PathBuf],
    beatmaps: &HashMap<&str, library::Beatmap<'_>>,
    mapping: Mapping,
    settings: &Settings,
) -> Report {
    let mut replays = Vec::new();
    let mut measured = Vec::new();
    let mut skipped = Vec::new();
    for path in paths {
        match hits::replay_samples(path, beatmaps) {
            Ok((header, samples)) => {
                if let Some(metrics) = fit::measure(&samples, settings) {
                    let entry = Entry {
                        played_at_ms: header.played_at_ms,
                        metrics,
                    };
                    measured.push((path.clone(), entry));
                }
                replays.push(samples);
            }
            Err(error) => skipped.push((path.clone(), format!("{error:#}"))),
        }
    }
    let usable: Vec<Sample> = replays
        .iter()
        .flatten()
        .copied()
        .filter(|sample| fit::is_usable(sample, settings))
        .collect();
    let suggestion = fit::suggest(&replays, mapping, settings);
    let edge_stop = suggestion.map_or(0.0, |suggestion| suggestion.edge_stop);
    Report {
        analyzed: replays.len(),
        skipped,
        measured,
        errors: usable.iter().map(aligned_error).collect(),
        sectors: sectors(&usable, 16, edge_stop),
        suggestion,
    }
}

// Runs of neighboring sectors with enough jumps to draw; a run covering every sector is a closed
// outline.
pub fn outline_runs(sectors: &[Sector]) -> Vec<Vec<Sector>> {
    let thin = |sector: &Sector| sector.samples < MIN_SECTOR_SAMPLES;
    let Some(gap) = sectors.iter().position(thin) else {
        return vec![sectors.to_vec()];
    };
    // Start at a gap so a run crossing 0° stays in one piece.
    let rotated = [&sectors[gap..], &sectors[..gap]].concat();
    rotated
        .split(thin)
        .filter(|run| !run.is_empty())
        .map(<[Sector]>::to_vec)
        .collect()
}

fn aligned_error(sample: &Sample) -> (f64, f64) {
    let (ux, uy) = sample.intended;
    let (ex, ey) = (sample.actual.0 - ux, sample.actual.1 - uy);
    let length = ux.hypot(uy) * sample.radius;
    ((ex * ux + ey * uy) / length, (ey * ux - ex * uy) / length)
}

fn sectors(samples: &[Sample], count: usize, edge_stop: f64) -> Vec<Sector> {
    let width = 360.0 / count as f64;
    let mut sums = vec![(0.0, 0.0, 0); count];
    for sample in samples {
        let (ux, uy) = sample.intended;
        let (vx, vy) = sample.actual;
        let direction = uy.atan2(ux).to_degrees().rem_euclid(360.0);
        let index = ((direction + width / 2.0) / width) as usize % count;
        let sum = &mut sums[index];
        let length = ux.hypot(uy);
        sum.0 += ((vx * ux + vy * uy) / length - edge_stop) / length;
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

        let sectors = sectors(&samples, 4, 0.0);

        assert_eq!(sectors[0].samples, 2);
        assert!((sectors[0].gain - 1.0).abs() < 1e-12);
        assert_eq!(sectors[1].direction, 90.0);
        assert!((sectors[1].gain - 0.95).abs() < 1e-12);
        assert_eq!(sectors[2].samples, 0);
        assert_eq!(sectors[2].gain, 0.0);
    }

    #[test]
    fn sector_gain_discounts_the_edge_stop() {
        let samples = [
            sample((200.0, 0.0), (190.0, 0.0)),
            sample((400.0, 0.0), (390.0, 0.0)),
        ];

        assert!((sectors(&samples, 4, -10.0)[0].gain - 1.0).abs() < 1e-12);
    }

    #[test]
    fn outline_runs_skip_thin_sectors_and_wrap_around() {
        let sector = |direction: f64, samples: usize| Sector {
            direction,
            gain: 1.0,
            turn: 0.0,
            samples,
        };
        let full = [
            sector(0.0, 20),
            sector(90.0, 20),
            sector(180.0, 3),
            sector(270.0, 20),
        ];

        let directions: Vec<Vec<f64>> = outline_runs(&full)
            .iter()
            .map(|run| run.iter().map(|s| s.direction).collect())
            .collect();

        assert_eq!(directions, [vec![270.0, 0.0, 90.0]]);
        assert_eq!(outline_runs(&[sector(0.0, 20), sector(90.0, 20)]).len(), 1);
        assert!(outline_runs(&[sector(0.0, 0), sector(90.0, 2)]).is_empty());
    }

    #[test]
    fn turned_moves_show_as_clockwise_turn() {
        let (sin, cos) = 3.0_f64.to_radians().sin_cos();
        let samples = [sample((200.0, 0.0), (200.0 * cos, 200.0 * sin))];

        assert!((sectors(&samples, 16, 0.0)[0].turn - 3.0).abs() < 1e-9);
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

        let report = analyze(
            std::slice::from_ref(&path),
            &HashMap::new(),
            mapping,
            &Settings::default(),
        );
        std::fs::remove_file(&path).unwrap();

        assert_eq!(report.analyzed, 0);
        assert_eq!(
            report.skipped,
            [(path, "beatmap not found in the osu! library".to_string())]
        );
        assert!(report.measured.is_empty());
        assert_eq!(report.suggestion, None);
    }
}
