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
